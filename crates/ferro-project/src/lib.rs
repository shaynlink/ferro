//! Project-manifest support for Ferro applications.
//!
//! A project is configured by a strict, versioned `ferro.toml` file. This crate owns parsing and
//! validation so the CLI and future target tooling agree on the public project contract.

use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::path::Path;

use ferro_sdk::{ApplicationDescriptor, Target};
use serde::Deserialize;

/// The only manifest schema accepted by this version of Ferro.
pub const CURRENT_SCHEMA: u32 = 1;

/// Validated configuration for a Ferro application project.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectManifest {
    application: ApplicationDescriptor,
    targets: Vec<Target>,
}

impl ProjectManifest {
    /// Validates and creates a project manifest.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError`] when the name, identifier, or target list is invalid.
    pub fn new(
        name: impl Into<String>,
        id: impl Into<String>,
        targets: Vec<Target>,
    ) -> Result<Self, ProjectError> {
        let name = name.into();
        validate_name(&name)?;
        let id = id.into();
        validate_application_id(&id)?;
        validate_targets(&targets)?;
        Ok(Self {
            application: ApplicationDescriptor::new(name, id),
            targets,
        })
    }

    /// Reads and validates `ferro.toml` at `path`.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError`] when the file cannot be read, parsed, or validated.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ProjectError> {
        let path = path.as_ref();
        let source = fs::read_to_string(path).map_err(|error| ProjectError::Read {
            path: path.to_owned(),
            error,
        })?;
        Self::parse(&source)
    }

    /// Parses and validates a manifest document.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError`] when the document has an unsupported schema or invalid content.
    pub fn parse(source: &str) -> Result<Self, ProjectError> {
        let raw: RawManifest = toml::from_str(source).map_err(ProjectError::Parse)?;
        if raw.schema != CURRENT_SCHEMA {
            return Err(ProjectError::UnsupportedSchema(raw.schema));
        }
        let targets = raw
            .app
            .targets
            .into_iter()
            .map(|target| {
                target
                    .parse::<Target>()
                    .map_err(|error| ProjectError::Invalid(error.to_string()))
            })
            .collect::<Result<_, _>>()?;
        Self::new(raw.app.name, raw.app.id, targets)
    }

    /// Returns the stable application metadata.
    #[must_use]
    pub fn application(&self) -> &ApplicationDescriptor {
        &self.application
    }

    /// Returns enabled targets in manifest order.
    #[must_use]
    pub fn targets(&self) -> &[Target] {
        &self.targets
    }

    /// Renders this manifest in Ferro's canonical TOML form.
    #[must_use]
    pub fn to_toml(&self) -> String {
        let targets = self
            .targets
            .iter()
            .map(|target| format!("\"{target}\""))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "schema = {CURRENT_SCHEMA}\n\n[app]\nname = \"{}\"\nid = \"{}\"\ntargets = [{targets}]\n",
            self.application.name(),
            self.application.id()
        )
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    schema: u32,
    app: RawApplication,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawApplication {
    name: String,
    id: String,
    targets: Vec<String>,
}

/// An error while loading or validating a Ferro project manifest.
#[derive(Debug)]
pub enum ProjectError {
    /// The manifest file could not be read.
    Read {
        /// Manifest path.
        path: std::path::PathBuf,
        /// Underlying filesystem error.
        error: std::io::Error,
    },
    /// The manifest is not valid TOML or has fields outside the schema.
    Parse(toml::de::Error),
    /// The manifest declares an unsupported schema number.
    UnsupportedSchema(u32),
    /// A semantic manifest invariant was violated.
    Invalid(String),
}

impl fmt::Display for ProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, error } => write!(
                formatter,
                "could not read Ferro manifest `{}`: {error}",
                path.display()
            ),
            Self::Parse(error) => write!(formatter, "invalid Ferro manifest: {error}"),
            Self::UnsupportedSchema(schema) => write!(
                formatter,
                "unsupported Ferro manifest schema {schema}; expected {CURRENT_SCHEMA}"
            ),
            Self::Invalid(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ProjectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { error, .. } => Some(error),
            Self::Parse(error) => Some(error),
            Self::UnsupportedSchema(_) | Self::Invalid(_) => None,
        }
    }
}

fn validate_name(name: &str) -> Result<(), ProjectError> {
    if name.trim().is_empty() {
        return Err(ProjectError::Invalid(
            "application name cannot be empty".to_owned(),
        ));
    }
    Ok(())
}

fn validate_application_id(id: &str) -> Result<(), ProjectError> {
    let valid = id.split('.').count() >= 3
        && id.split('.').all(|component| {
            !component.is_empty()
                && component
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
        });
    if !valid {
        return Err(ProjectError::Invalid(
            "application id must contain at least three dot-separated ASCII labels".to_owned(),
        ));
    }
    Ok(())
}

fn validate_targets(targets: &[Target]) -> Result<(), ProjectError> {
    if targets.is_empty() {
        return Err(ProjectError::Invalid(
            "at least one desktop target must be declared".to_owned(),
        ));
    }
    let unique = targets.iter().copied().collect::<HashSet<_>>();
    if unique.len() != targets.len() {
        return Err(ProjectError::Invalid(
            "a Ferro manifest cannot declare a target more than once".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use ferro_sdk::Target;

    use super::{CURRENT_SCHEMA, ProjectManifest};

    #[test]
    fn parses_a_desktop_manifest() {
        let manifest = ProjectManifest::parse(
            "schema = 1\n\n[app]\nname = \"Atlas\"\nid = \"dev.ferro.atlas\"\ntargets = [\"macos\", \"linux\"]\n",
        )
        .expect("valid manifest");

        assert_eq!(manifest.application().name(), "Atlas");
        assert_eq!(manifest.targets(), [Target::Macos, Target::Linux]);
    }

    #[test]
    fn rejects_unknown_fields() {
        let error = ProjectManifest::parse(
            "schema = 1\nextra = true\n\n[app]\nname = \"Atlas\"\nid = \"dev.ferro.atlas\"\ntargets = [\"macos\"]\n",
        )
        .unwrap_err();

        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn rejects_unknown_and_duplicate_targets() {
        assert!(ProjectManifest::parse(
            "schema = 1\n\n[app]\nname = \"Atlas\"\nid = \"dev.ferro.atlas\"\ntargets = [\"ios\"]\n",
        )
        .is_err());
        assert!(
            ProjectManifest::new(
                "Atlas",
                "dev.ferro.atlas",
                vec![Target::Macos, Target::Macos],
            )
            .is_err()
        );
    }

    #[test]
    fn renders_the_current_schema() {
        let manifest = ProjectManifest::new(
            "Atlas",
            "dev.ferro.atlas",
            vec![Target::Macos, Target::Windows, Target::Linux],
        )
        .expect("valid manifest");

        assert!(
            manifest
                .to_toml()
                .starts_with(&format!("schema = {CURRENT_SCHEMA}"))
        );
    }
}

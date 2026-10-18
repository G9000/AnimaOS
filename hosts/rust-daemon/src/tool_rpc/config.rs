//! Explicit activation only; never discover an interpreter from PATH/profiles.
use std::{ffi::OsString, path::PathBuf};

pub(crate) const PYTHON_PATH_ENV: &str = "ANIMAOS_RS_TOOL_RPC_PYTHON_PATH";

#[derive(Clone, Default)]
pub(crate) struct ToolRpcConfig {
    pub(crate) interpreter: Option<PathBuf>,
}

impl ToolRpcConfig {
    pub(crate) fn parse(value: Option<OsString>) -> Result<Self, &'static str> {
        let Some(value) = value else {
            return Ok(Self::default());
        };
        let path = PathBuf::from(value);
        if !path.is_absolute() || !path.is_file() {
            return Err(
                "ANIMAOS_RS_TOOL_RPC_PYTHON_PATH must select an existing absolute interpreter file",
            );
        }
        Ok(Self {
            interpreter: Some(path),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fp06_config_absent_is_disabled_without_discovery() {
        let config = ToolRpcConfig::parse(None).expect("absence is an explicit disabled state");
        assert!(config.interpreter.is_none());
    }

    #[test]
    fn fp06_config_only_explicit_existing_absolute_file_is_accepted() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("selected-python.exe");
        std::fs::write(&executable, b"synthetic nonexecuted interpreter").unwrap();
        let config = ToolRpcConfig::parse(Some(executable.clone().into_os_string()))
            .expect("explicit file is config only, without starting it");
        assert_eq!(config.interpreter, Some(executable));
        for invalid in [
            OsString::new(),
            OsString::from("python.exe"),
            directory.path().as_os_str().to_owned(),
            directory.path().join("absent.exe").into_os_string(),
        ] {
            assert!(ToolRpcConfig::parse(Some(invalid)).is_err());
        }
    }
}

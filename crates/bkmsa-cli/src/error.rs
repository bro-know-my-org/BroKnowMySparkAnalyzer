use std::process::ExitCode;

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("configuration error: {0}")]
    Config(String),
    #[error("unable to read report: {0}")]
    Input(String),
    #[error("unable to decode report: {0}")]
    Decode(String),
    #[error("analysis provider failed: {0}")]
    Provider(String),
    #[error("analysis failed: {0}")]
    Analysis(String),
    #[error("unable to write output: {0}")]
    Output(String),
}

impl CliError {
    pub fn exit_code(&self) -> ExitCode {
        ExitCode::from(match self {
            Self::Config(_) => 2,
            Self::Input(_) => 3,
            Self::Decode(_) => 4,
            Self::Provider(_) => 5,
            Self::Analysis(_) | Self::Output(_) => 6,
        })
    }
}

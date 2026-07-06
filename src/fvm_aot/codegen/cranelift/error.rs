use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub(in crate::fvm_aot) enum CodegenError {
    Verify {
        function: String,
        source: anyhow::Error,
    },
    Unsupported {
        function: String,
        category: &'static str,
        detail: String,
    },
    Backend {
        function: String,
        message: String,
    },
}

impl fmt::Display for CodegenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verify { function, source } => {
                write!(
                    formatter,
                    "phase=verify function={function} message={source}"
                )
            }
            Self::Unsupported {
                function,
                category,
                detail,
            } => write!(
                formatter,
                "phase=unsupported-codegen function={function} instruction={category} message={detail}"
            ),
            Self::Backend { function, message } => {
                write!(
                    formatter,
                    "phase=cranelift-object function={function} message={message}"
                )
            }
        }
    }
}

impl Error for CodegenError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Verify { source, .. } => Some(source.as_ref()),
            Self::Unsupported { .. } | Self::Backend { .. } => None,
        }
    }
}

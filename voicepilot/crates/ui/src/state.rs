use std::sync::Arc;
use trust_kernel::kernel::TrustKernel;

pub struct AppState {
    pub kernel: Arc<TrustKernel>,
}

impl AppState {
    pub fn new(kernel: TrustKernel) -> Self {
        Self {
            kernel: Arc::new(kernel),
        }
    }

    pub fn new_in_memory() -> anyhow::Result<Self> {
        let kernel = TrustKernel::open_in_memory()?;
        Ok(Self::new(kernel))
    }

    pub fn new_file(path: &str) -> anyhow::Result<Self> {
        let kernel = TrustKernel::open_file(path)?;
        Ok(Self::new(kernel))
    }
}

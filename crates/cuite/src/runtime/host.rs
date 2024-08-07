use super::Runtime;

pub struct HostRuntime;

impl Runtime for HostRuntime {
    fn id(&self) -> u64 {
        0
    }
}

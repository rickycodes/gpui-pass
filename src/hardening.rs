use crate::Error;

/// Applied once at desktop startup, before any store is opened.
pub fn harden_process() -> Result<(), Error> {
    #[cfg(target_os = "linux")]
    {
        use rustix::process::{
            DumpableBehavior, Resource, Rlimit, set_dumpable_behavior, setrlimit,
        };
        setrlimit(
            Resource::Core,
            Rlimit {
                current: Some(0),
                maximum: Some(0),
            },
        )
        .map_err(std::io::Error::from)?;
        set_dumpable_behavior(DumpableBehavior::NotDumpable).map_err(std::io::Error::from)?;
    }
    Ok(())
}

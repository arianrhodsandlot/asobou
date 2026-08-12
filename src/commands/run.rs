pub use crate::game_session::Launch as RunConfig;

pub fn run(config: RunConfig) -> Result<(), Box<dyn std::error::Error>> {
    match crate::game_session::run(config) {
        Ok(crate::game_session::Outcome::Completed) => Ok(()),
        Ok(crate::game_session::Outcome::LaunchRejected(rom)) => {
            eprintln!("Failed to load ROM: {}", rom.display());
            Ok(())
        }
        Err(error) => match error.downcast::<crate::game_session::LaunchFailure>() {
            Ok(error) => {
                eprintln!("Error: {error}");
                std::process::exit(1);
            }
            Err(error) => Err(error),
        },
    }
}

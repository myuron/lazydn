/// Runs effects as tokio tasks for the event loop.
pub mod exec;
/// Runs the API calls that the app asks for.
pub mod executor;
/// Starts the browser and writes to the clipboard, behind traits so that tests start nothing.
pub mod external;
/// The app together with the parts that act on its effects.
pub mod run;
/// Prepares and finishes the sign-in: the device key, the approval URL and the registration.
pub mod sign_in;
/// Decides the first session from the device key and builds the app from what main found.
pub mod startup;

#[cfg(test)]
mod exec_tests;
#[cfg(test)]
mod executor_tests;
#[cfg(test)]
mod external_tests;
#[cfg(test)]
mod run_tests;
#[cfg(test)]
mod sign_in_tests;
#[cfg(test)]
mod startup_tests;

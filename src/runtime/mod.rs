/// Runs the API calls that the app asks for.
pub mod executor;
/// Starts the browser and writes to the clipboard, behind traits so that tests start nothing.
pub mod external;
/// Prepares and finishes the sign-in: the device key, the approval URL and the registration.
pub mod sign_in;

#[cfg(test)]
mod executor_tests;
#[cfg(test)]
mod external_tests;
#[cfg(test)]
mod sign_in_tests;

//! `--flag value` parsing, shared by the binaries that take any.

pub struct Args(Vec<String>);

impl Args {
    pub fn new() -> Args {
        Args(std::env::args().collect())
    }

    pub fn get(&self, key: &str) -> Option<String> {
        self.0.iter().position(|a| a == key).and_then(|i| self.0.get(i + 1)).cloned()
    }
}

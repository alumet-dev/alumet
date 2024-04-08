use std::num::ParseIntError;

/// The current version of the alumet crate, for checking purposes.
const ALUMET_VERSION: &str = env!("CARGO_PKG_VERSION");

/// A version number that follows semantic versioning.
/// 
/// See [`Version::parse`].
pub struct Version {
    x: u8,
    y: u8,
    z: u8,
}

#[derive(Debug)]
pub enum Error {
    Parse(ParseIntError),
    Invalid,
}

impl Version {
    /// Returns the current version of ALUMET, as specified in Cargo.toml.
    pub fn alumet() -> Version {
        Self::parse(ALUMET_VERSION).unwrap()
    }

    /// Parses a version number of the form `"x.y.z"` where x,y,z are integers.
    /// 
    /// It is allowed to omit the last number, in which case `z` is inferred to zero.
    /// For example, `"1.0"` is a valid version and is considered to be equal to `"1.0.0"`.
    /// 
    /// ## Example
    /// ```ignore
    /// let version = Version::parse("1.0.2").expect("the version number should be valid");
    /// ```
    pub fn parse(version_string: &str) -> Result<Version, Error> {
        let mut parts: Vec<&str> = version_string.split('.').collect();
        match parts.len() {
            0 | 1 => return Err(Error::Invalid),
            2 => parts.push("0"), // a.b => a.b.0
            _ => (),
        };
        let parts: Result<Vec<u8>, ParseIntError> = parts.into_iter().map(|p| p.parse()).collect();
        let parts = parts.map_err(|e| Error::Parse(e))?;
        Ok(Version {
            x: parts[0],
            y: parts[1],
            z: parts[2],
        })
    }

    /// Checks if a plugin that requires version `required_version` can be loaded with version `self`.
    /// 
    /// The check is based on semantic versioning, see https://doc.rust-lang.org/cargo/reference/semver.html
    pub fn can_load(&self, required_version: &Version) -> bool {
        // 0.0.z: a change of z is always a major change
        // 0.y.z: a change of y is a major change
        // x.y.z: usual major.minor.patch
        match (self.x, self.y, self.z) {
            (0, 0, z) => required_version.x == 0 && required_version.y == 0 && required_version.z == z,
            (0, y, z) => required_version.x == 0 && required_version.y == y && required_version.z <= z,
            (x, y, _z) => required_version.x == x && required_version.y <= y,
        }
    }
}

impl std::fmt::Debug for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "v{}.{}.{}", self.x, self.y, self.z)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Parse(err) => write!(f, "version part could not be parsed: {}", err),
            Error::Invalid => f.write_str("invalid version format, please use \"x.y.z\" with integers"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Version;

    #[test]
    pub fn parsing() {
        // valid
        Version::parse("10.33.111").unwrap();
        Version::parse("1.2.3").unwrap();
        Version::parse("1.2").unwrap();

        // invalid
        assert!(Version::parse("").is_err());
        assert!(Version::parse("1").is_err());
        assert!(Version::parse("123456789.2").is_err());
        assert!(Version::parse("1.123456789").is_err());
        assert!(Version::parse("1.2.123456789").is_err());
        assert!(Version::parse("a.b.c").is_err());
        assert!(Version::parse("1.0.1-beta").is_err());
        assert!(Version::parse("1.0.1b572").is_err());
    }

    #[test]
    pub fn compatibility_comparison() {
        // ------ with 1.x.y ------
        let base = Version::parse("1.0.7").unwrap();
        // compatible
        assert!(base.can_load(&Version::parse("1.0.0").unwrap()));
        assert!(base.can_load(&Version::parse("1.0.4").unwrap()));
        assert!(base.can_load(&Version::parse("1.0.7").unwrap()));
        assert!(base.can_load(&Version::parse("1.0.11").unwrap()));

        // not compatible
        assert!(!base.can_load(&Version::parse("1.1.0").unwrap()));
        assert!(!base.can_load(&Version::parse("2.0.0").unwrap()));
        assert!(!base.can_load(&Version::parse("2.3.0").unwrap()));
        assert!(!base.can_load(&Version::parse("2.3.4").unwrap()));

        // ------ with 0.x.y ------
        let base = Version::parse("0.1.7").unwrap();
        // compatible
        assert!(base.can_load(&Version::parse("0.1.7").unwrap()));
        assert!(base.can_load(&Version::parse("0.1.6").unwrap()));
        assert!(base.can_load(&Version::parse("0.1.0").unwrap()));

        // not compatible
        assert!(!base.can_load(&Version::parse("0.1.8").unwrap()));
        assert!(!base.can_load(&Version::parse("0.1.222").unwrap()));
        assert!(!base.can_load(&Version::parse("0.2.0").unwrap()));
        assert!(!base.can_load(&Version::parse("0.2.7").unwrap()));
        assert!(!base.can_load(&Version::parse("1.0.0").unwrap()));
        assert!(!base.can_load(&Version::parse("1.2.0").unwrap()));
        assert!(!base.can_load(&Version::parse("1.1.7").unwrap()));
    }
}

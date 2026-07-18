//! Pure, network-free update strategy and canonical release destinations.

pub const REPOSITORY_URL: &str = "https://github.com/themoretheless/auto-reverse";
pub const LATEST_RELEASE_URL: &str =
    "https://github.com/themoretheless/auto-reverse/releases/latest";
pub const ALL_RELEASES_URL: &str = "https://github.com/themoretheless/auto-reverse/releases";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseChannel {
    LatestStable,
    IncludingPrereleases,
}

impl ReleaseChannel {
    pub const fn url(self) -> &'static str {
        match self {
            Self::LatestStable => LATEST_RELEASE_URL,
            Self::IncludingPrereleases => ALL_RELEASES_URL,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::LatestStable => "latest stable release",
            Self::IncludingPrereleases => "all releases, including prereleases",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdatePolicy {
    pub channel: ReleaseChannel,
    /// Retained only to explain an old config request. The current strategy
    /// never turns this into background network activity.
    pub legacy_automatic_check_requested: bool,
}

impl UpdatePolicy {
    pub const fn from_legacy_flags(check_for_updates: bool, include_beta_updates: bool) -> Self {
        Self {
            channel: if include_beta_updates {
                ReleaseChannel::IncludingPrereleases
            } else {
                ReleaseChannel::LatestStable
            },
            legacy_automatic_check_requested: check_for_updates,
        }
    }

    pub const fn strategy_label(self) -> &'static str {
        "manual browser check; no background network requests"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_and_prerelease_destinations_are_explicit() {
        assert_eq!(ReleaseChannel::LatestStable.url(), LATEST_RELEASE_URL);
        assert_eq!(ReleaseChannel::IncludingPrereleases.url(), ALL_RELEASES_URL);
        assert!(LATEST_RELEASE_URL.starts_with(REPOSITORY_URL));
        assert!(ALL_RELEASES_URL.starts_with(REPOSITORY_URL));
    }

    #[test]
    fn legacy_flags_never_enable_background_checks() {
        let policy = UpdatePolicy::from_legacy_flags(true, true);

        assert!(policy.legacy_automatic_check_requested);
        assert_eq!(policy.channel, ReleaseChannel::IncludingPrereleases);
        assert_eq!(
            policy.strategy_label(),
            "manual browser check; no background network requests"
        );
    }
}

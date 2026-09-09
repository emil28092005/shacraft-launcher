//! The same origin policy applies to initial artifact URLs and every redirect.
use reqwest::{blocking::Client, redirect::Policy};
use std::time::Duration;
use url::Url;

pub(crate) fn allows(value: &str, hosts: &[&str]) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && url.host_str().is_some_and(|host| hosts.contains(&host))
}

pub(crate) fn client(
    hosts: &'static [&'static str],
    timeout: Duration,
) -> Result<Client, reqwest::Error> {
    Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(15))
        .timeout(timeout)
        .redirect(Policy::custom(move |attempt| {
            if attempt.previous().len() >= 10 {
                attempt.error("too many redirects")
            } else if allows(attempt.url().as_str(), hosts) {
                attempt.follow()
            } else {
                attempt.error("redirect leaves the trusted download hosts")
            }
        }))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_https_origins_reject_authority_ambiguity_and_cross_domain_redirects() {
        let hosts = ["piston-meta.mojang.com"];
        assert!(allows("https://piston-meta.mojang.com/game.json", &hosts));
        for url in [
            "http://piston-meta.mojang.com/game.json",
            "https://piston-meta.mojang.com.attacker.test/game.json",
            "https://user@piston-meta.mojang.com/game.json",
            "https://piston-meta.mojang.com:444/game.json",
            "https://maven.neoforged.net/game.json",
        ] {
            assert!(!allows(url, &hosts), "{url}");
        }
    }
}

use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct TrackAuthConfig {
    enforce_track_auth: bool,
    track_tokens: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackAuthError {
    MissingToken,
    InvalidToken,
    ForbiddenTrack,
}

impl TrackAuthConfig {
    pub fn new(enforce_track_auth: bool, track_tokens: HashMap<String, String>) -> Self {
        Self {
            enforce_track_auth,
            track_tokens,
        }
    }

    pub fn disabled() -> Self {
        Self::default()
    }

    pub fn enforce_track_auth(&self) -> bool {
        self.enforce_track_auth
    }

    pub fn token_count(&self) -> usize {
        self.track_tokens.len()
    }

    pub fn authorize_track_token(
        &self,
        track_id: &str,
        auth_token: Option<&str>,
    ) -> Result<(), TrackAuthError> {
        if !self.enforce_track_auth {
            return Ok(());
        }

        let token = auth_token
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .ok_or(TrackAuthError::MissingToken)?;

        match self.track_tokens.get(track_id) {
            Some(expected) if expected == token => Ok(()),
            _ => {
                if self
                    .track_tokens
                    .values()
                    .any(|candidate| candidate == token)
                {
                    Err(TrackAuthError::ForbiddenTrack)
                } else {
                    Err(TrackAuthError::InvalidToken)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> TrackAuthConfig {
        let mut map = HashMap::new();
        map.insert("track-a".to_string(), "token-a".to_string());
        map.insert("track-b".to_string(), "token-b".to_string());
        TrackAuthConfig::new(true, map)
    }

    #[test]
    fn disabled_auth_allows_missing_token() {
        let auth = TrackAuthConfig::disabled();
        assert!(auth.authorize_track_token("track-a", None).is_ok());
    }

    #[test]
    fn enabled_auth_requires_token() {
        let auth = cfg();
        assert_eq!(
            auth.authorize_track_token("track-a", None),
            Err(TrackAuthError::MissingToken)
        );
    }

    #[test]
    fn enabled_auth_rejects_invalid_token() {
        let auth = cfg();
        assert_eq!(
            auth.authorize_track_token("track-a", Some("bad-token")),
            Err(TrackAuthError::InvalidToken)
        );
    }

    #[test]
    fn enabled_auth_rejects_wrong_track_token() {
        let auth = cfg();
        assert_eq!(
            auth.authorize_track_token("track-a", Some("token-b")),
            Err(TrackAuthError::ForbiddenTrack)
        );
    }

    #[test]
    fn enabled_auth_accepts_scoped_token() {
        let auth = cfg();
        assert!(
            auth.authorize_track_token("track-a", Some("token-a"))
                .is_ok()
        );
    }
}

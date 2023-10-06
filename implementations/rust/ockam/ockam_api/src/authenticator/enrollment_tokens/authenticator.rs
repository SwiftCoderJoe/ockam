use lru::LruCache;
use ockam_core::compat::sync::{Arc, RwLock};
use std::num::NonZeroUsize;
use std::time::Duration;

use crate::authenticator::enrollment_tokens::types::Token;
use crate::authenticator::enrollment_tokens::{EnrollmentTokenAcceptor, EnrollmentTokenIssuer};
use crate::authenticator::MembersStorage;

pub(super) const MAX_TOKEN_DURATION: Duration = Duration::from_secs(600);

#[derive(Clone)]
pub struct EnrollmentTokenAuthenticator {
    // TODO: Replace with something sane and standard + implement expiration
    pub(super) tokens: Arc<RwLock<LruCache<[u8; 32], Token>>>,
}

impl EnrollmentTokenAuthenticator {
    pub fn new_worker_pair(
        members_storage: Arc<dyn MembersStorage>,
    ) -> (EnrollmentTokenIssuer, EnrollmentTokenAcceptor) {
        let base = Self {
            tokens: Arc::new(RwLock::new(LruCache::new(
                NonZeroUsize::new(128).expect("0 < 128"),
            ))),
        };
        (
            EnrollmentTokenIssuer::new(base.clone()),
            EnrollmentTokenAcceptor::new(base, members_storage),
        )
    }
}

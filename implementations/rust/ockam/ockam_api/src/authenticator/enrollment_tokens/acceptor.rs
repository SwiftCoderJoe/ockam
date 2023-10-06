use minicbor::Decoder;
use ockam::identity::utils::now;
use ockam::identity::IdentitySecureChannelLocalInfo;
use ockam_core::api::{Method, RequestHeader, Response};
use ockam_core::compat::sync::Arc;
use ockam_core::{Result, Routed, Worker};
use ockam_node::Context;
use tracing::trace;

use crate::authenticator::enrollment_tokens::EnrollmentTokenAuthenticator;
use crate::authenticator::one_time_code::OneTimeCode;
use crate::authenticator::{secure_channel_required, Member, MembersStorage};

pub struct EnrollmentTokenAcceptor {
    pub(super) authenticator: EnrollmentTokenAuthenticator,
    pub(super) members_storage: Arc<dyn MembersStorage>,
}

impl EnrollmentTokenAcceptor {
    pub fn new(
        authenticator: EnrollmentTokenAuthenticator,
        members_storage: Arc<dyn MembersStorage>,
    ) -> Self {
        Self {
            authenticator,
            members_storage,
        }
    }
}

#[ockam_core::worker]
impl Worker for EnrollmentTokenAcceptor {
    type Context = Context;
    type Message = Vec<u8>;

    async fn handle_message(&mut self, c: &mut Context, m: Routed<Self::Message>) -> Result<()> {
        if let Ok(i) = IdentitySecureChannelLocalInfo::find_info(m.local_message()) {
            let from = i.their_identity_id();
            let mut dec = Decoder::new(m.as_body());
            let req: RequestHeader = dec.decode()?;
            trace! {
                target: "ockam_api::authenticator::direct::enrollment_token_acceptor",
                from   = %from,
                id     = %req.id(),
                method = ?req.method(),
                path   = %req.path(),
                body   = %req.has_body(),
                "request"
            }
            let res = match (req.method(), req.path()) {
                (Some(Method::Post), "/") | (Some(Method::Post), "/credential") => {
                    //TODO: move out of the worker handle_message implementation
                    let otc: OneTimeCode = dec.decode()?;
                    let token = match self.authenticator.tokens.write() {
                        Ok(mut r) => {
                            if let Some(tkn) = r.pop(otc.code()) {
                                if tkn.time.elapsed() > tkn.max_token_duration {
                                    Err(Response::forbidden(&req, "expired token"))
                                } else {
                                    Ok(tkn)
                                }
                            } else {
                                Err(Response::forbidden(&req, "unknown token"))
                            }
                        }
                        Err(_) => Err(Response::internal_error(
                            &req,
                            "Failed to get read lock on tokens table",
                        )),
                    };
                    match token {
                        Ok(tkn) => {
                            //TODO: fixme:  unify use of hashmap vs btreemap
                            let attrs = tkn
                                .attrs
                                .iter()
                                .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
                                .collect();
                            let member =
                                Member::new(from, attrs, Some(tkn.generated_by), now()?, false);
                            self.members_storage.add_member(member).await?;
                            Response::ok(&req).to_vec()?
                        }
                        Err(err) => err.to_vec()?,
                    }
                }
                _ => Response::unknown_path(&req).to_vec()?,
            };
            c.send(m.return_route(), res).await
        } else {
            secure_channel_required(c, m).await
        }
    }
}

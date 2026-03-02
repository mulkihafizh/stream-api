use crate::config::Config;
use actix_web::dev::ServiceRequest;
use actix_web::web;
use actix_web::Error;
use actix_web_httpauth::extractors::bearer::BearerAuth;

pub async fn validator(
    req: ServiceRequest,
    credentials: BearerAuth,
) -> Result<ServiceRequest, (Error, ServiceRequest)> {
    let config = req
        .app_data::<web::Data<Config>>()
        .expect("Config not found in app_data");

    if credentials.token() == config.bearer_token {
        Ok(req)
    } else {
        let err = actix_web::error::ErrorUnauthorized("Invalid or missing bearer token");
        Err((err, req))
    }
}

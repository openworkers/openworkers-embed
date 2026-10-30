use openworkers_core::{HttpResponse, ResponseBody};

pub(crate) fn text(status: u16, body: &str) -> HttpResponse {
    reply(status, "text/plain; charset=utf-8", body.to_string())
}

pub(crate) fn reply(status: u16, content_type: &str, body: String) -> HttpResponse {
    HttpResponse {
        status,
        headers: vec![("content-type".to_string(), content_type.to_string())],
        body: ResponseBody::Bytes(body.into()),
    }
}

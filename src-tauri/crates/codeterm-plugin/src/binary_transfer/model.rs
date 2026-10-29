use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub const MAX_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_TIMEOUT_MS: u64 = 120_000;
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields, rename_all="camelCase")]
pub struct BinaryTransferDescriptor {
    pub max_upload_bytes: u64,
    pub max_download_bytes: u64,
    pub timeout_ms: u64,
    #[serde(skip_serializing_if="BTreeMap::is_empty")]
    pub credential_origins:BTreeMap<String,Vec<String>>,
    #[serde(skip_serializing_if="Vec::is_empty")]
    pub request_headers:Vec<String>,
}
impl Default for BinaryTransferDescriptor {
    fn default()->Self {Self {max_upload_bytes:16*1024*1024,max_download_bytes:64*1024*1024,timeout_ms:30_000,credential_origins:BTreeMap::new(),request_headers:Vec::new()}}
}
impl BinaryTransferDescriptor {
    pub fn valid(&self) -> bool { self.max_upload_bytes <= MAX_BYTES && self.max_download_bytes > 0 && self.max_download_bytes <= MAX_BYTES && self.timeout_ms > 0 && self.timeout_ms <= MAX_TIMEOUT_MS && self.request_headers.len()<=16 && self.request_headers.iter().enumerate().all(|(i,name)|!forbidden_header(name)&&name.len()<=128&&reqwest::header::HeaderName::from_bytes(name.as_bytes()).is_ok()&&!self.request_headers[..i].iter().any(|prior|prior.eq_ignore_ascii_case(name))) && self.credential_origins.len()<=16 && self.credential_origins.iter().all(|(id,origins)|{
        (id.strip_prefix("secret:").or_else(||id.strip_prefix("declared:")).is_some_and(|suffix|!suffix.is_empty())) && id.len()<=160 && !origins.is_empty() && origins.len()<=8 && origins.iter().enumerate().all(|(i,origin)|{
            origin.len()<=256 && !origins[..i].contains(origin) && reqwest::Url::parse(origin).is_ok_and(|url|super::network::validate_url(&url).is_ok()&&url.origin().ascii_serialization()==*origin)
        })
    }) }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all="camelCase")]
pub struct TransferRequest {
    pub url: String,
    pub method: String,
    #[serde(default)] pub headers: BTreeMap<String,String>,
    pub credential: Option<Credential>,
    pub body: Option<InputBody>,
    pub response: Output,
    pub timeout_ms: Option<u64>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag="kind", rename_all="snake_case", deny_unknown_fields)]
pub enum Credential {
    Secret { name: String, header: String, prefix: Option<String> },
    Declared { id: String, headers: BTreeMap<String,String> },
}
impl Credential {pub fn binding_key(&self)->String {match self {Self::Secret {name,..}=>format!("secret:{name}"),Self::Declared {id,..}=>format!("declared:{id}")}}}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag="kind", rename_all="snake_case", deny_unknown_fields)]
pub enum Input { Owned { #[serde(default,skip_serializing_if="Option::is_none")] identity: Option<FileIdentity>, path: String, #[serde(rename="contentType")] content_type: String, #[serde(rename="fileName")] file_name: String }, Selected { handle: String } }
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag="kind", rename_all="snake_case", deny_unknown_fields)]
pub enum InputBody { File { file: Input }, Multipart { parts: Vec<Part> } }
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag="kind", rename_all="snake_case", deny_unknown_fields)]
pub enum Part { Text { name: String, value: String }, File { name: String, file: Input } }
pub const MAX_METADATA_BYTES:u64=256*1024;
#[derive(Debug,Clone,PartialEq,Eq,Deserialize,Serialize)]
#[serde(deny_unknown_fields)]
pub struct FileIdentity {pub size:u64,pub sha256:String}
#[derive(Debug,Clone,Deserialize,Serialize)]
#[serde(tag="kind",rename_all="snake_case",deny_unknown_fields)]
pub enum Output {
    File {#[serde(rename="fileName")] file_name:String,#[serde(rename="contentType")] content_type:String,#[serde(rename="maxBytes")] max_bytes:u64,sha256:Option<String>,statuses:Vec<u16>},
    Json {#[serde(rename="maxBytes")] max_bytes:u64,statuses:Vec<u16>},
    Empty {statuses:Vec<u16>},
}
impl Output {
    pub fn statuses(&self)->&[u16] {match self {Self::File {statuses,..}|Self::Json {statuses,..}|Self::Empty {statuses}=>statuses}}
    pub fn max_bytes(&self)->u64 {match self {Self::File {max_bytes,..}|Self::Json {max_bytes,..}=>*max_bytes,Self::Empty {..}=>0}}
    pub fn validate(&self,limit:u64)->Result<()> {
        let statuses=self.statuses();
        if statuses.is_empty() || statuses.len()>8 || statuses.iter().enumerate().any(|(i,status)|!(200..=599).contains(status)||(300..400).contains(status)||statuses[..i].contains(status)) {return Err(TransferRefusal::InvalidRequest);}
        match self {
            Self::File {file_name,content_type,max_bytes,sha256,..}=>{if file_name.len()>512 || *max_bytes==0 || *max_bytes>limit || sha256.as_ref().is_some_and(|value|!crate::media_delivery::valid_token(value)){return Err(TransferRefusal::InvalidRequest);}super::upload::safe_name(file_name,content_type)?;},
            Self::Json {max_bytes,..}=>{if *max_bytes==0 || *max_bytes>limit.min(MAX_METADATA_BYTES){return Err(TransferRefusal::InvalidRequest);}},
            Self::Empty {..}=>{},
        }
        Ok(())
    }
}
#[derive(Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct TransferInputGrant {pub handle:String,pub file_name:String,pub content_type:String,pub size:u64,pub sha256:String}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all="snake_case")]
pub enum TransferRefusal { InvalidRequest, Unavailable, NotAllowed, Aborted, Deadline, TooLarge, MimeMismatch, DigestMismatch, FileChanged, HttpStatus, RedirectDenied, TransportUnconfirmed, InvalidResponse, Storage }
#[derive(Debug, Serialize)]
#[serde(rename_all="camelCase")]
pub struct TransferFile { pub path: String, pub file_name: String, pub content_type: String, pub size: u64, pub sha256: String }
#[derive(Debug, Serialize)]
#[serde(tag="kind",rename_all="snake_case")]
pub enum TransferReceipt {
    File {status:u16,file:TransferFile},
    Json {status:u16,data:serde_json::Value,size:u64,sha256:String},
    Empty {status:u16},
}
impl TransferReceipt {pub fn file(&self)->Option<&TransferFile>{if let Self::File {file,..}=self{Some(file)}else{None}}}
#[derive(Debug, Serialize)]
#[serde(rename_all="camelCase")]
pub struct TransferFailure { pub error: TransferRefusal, pub request_may_have_started: bool, pub status: Option<u16> }
pub type Result<T> = std::result::Result<T,TransferRefusal>;

pub fn forbidden_header(name:&str)->bool {
    matches!(name.to_ascii_lowercase().as_str(),"authorization"|"proxy-authorization"|"cookie"|"cookie2"|"set-cookie"|"host"|"content-length"|"transfer-encoding"|"connection"|"proxy-connection"|"te"|"trailer"|"upgrade"|"expect"|"x-api-key"|"api-key")
}

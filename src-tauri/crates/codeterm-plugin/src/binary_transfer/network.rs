use super::{model::*,upload::{ByteStream,Upload,CHUNK}};
use async_trait::async_trait;
use futures_util::{stream,StreamExt};
use reqwest::{header::HeaderMap,Url};
use std::{net::{IpAddr,SocketAddr,ToSocketAddrs},sync::{Arc,atomic::{AtomicUsize,Ordering}}};

pub fn validate_url(url:&Url)->Result<()> {
    if url.scheme()!="https" || !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() || url.as_str().len()>8192 { return Err(TransferRefusal::NotAllowed); }
    let host=url.host_str().ok_or(TransferRefusal::NotAllowed)?.trim_matches(['[',']']).trim_end_matches('.');
    if host=="localhost" || host.ends_with(".localhost") || host.ends_with(".local") { return Err(TransferRefusal::NotAllowed); }
    if let Ok(ip)=host.parse::<IpAddr>() { if !public_ip(ip) { return Err(TransferRefusal::NotAllowed); } }
    Ok(())
}
pub fn exact_grant(url:&Url,allowed:&[String])->bool {
    let Some(host)=url.host_str() else{return false};
    let Some(port)=url.port_or_known_default() else{return false};
    let plain=codeterm_util::network_host::normalize_network_host(host).ok();
    let full=codeterm_util::network_host::normalize_network_host(&format!("{host}:{port}")).ok();
    allowed.iter().filter_map(|value|codeterm_util::network_host::normalize_network_host(value).ok()).any(|entry|Some(&entry)==full.as_ref() || (port==443 && Some(&entry)==plain.as_ref()))
}
pub fn public_ip(ip:IpAddr)->bool {
    match ip {
        IpAddr::V4(ip)=>{
            let [a,b,c,_]=ip.octets();
            !(a==0 || a==10 || a==127 || a>=224 || (a==100 && (64..=127).contains(&b)) || (a==169&&b==254) || (a==172&&(16..=31).contains(&b)) || (a==192&&(b==168 || (b==0&&(c==0||c==2)) || (b==88&&c==99))) || (a==198&&(b==18||b==19||(b==51&&c==100))) || (a==203&&b==0&&c==113))
        },
        IpAddr::V6(ip)=>{
            if let Some(v4)=ip.to_ipv4_mapped(){return public_ip(IpAddr::V4(v4));}
            let words=ip.segments();
            words[0]&0xe000==0x2000 && words[0]!=0x2002 && !(words[0]==0x2001 && matches!(words[1],0|0x10|0x20|0xdb8))
        }
    }
}
static DNS_WORKERS:AtomicUsize=AtomicUsize::new(0);
struct DnsPermit;
impl Drop for DnsPermit {fn drop(&mut self){DNS_WORKERS.fetch_sub(1,Ordering::AcqRel);}}
struct PublicDns;
impl reqwest::dns::Resolve for PublicDns {
    fn resolve(&self,name:reqwest::dns::Name)->reqwest::dns::Resolving {
        let name=name.as_str().to_owned();
        Box::pin(async move {
            DNS_WORKERS.fetch_update(Ordering::AcqRel,Ordering::Acquire,|count|(count<8).then_some(count+1)).map_err(|_|std::io::Error::from(std::io::ErrorKind::WouldBlock))?;
            let permit=DnsPermit;
            let addresses=tokio::task::spawn_blocking(move || {
                let _permit=permit;
                (name.as_str(),0).to_socket_addrs().map(|addresses|addresses.take(33).collect::<Vec<SocketAddr>>())
            }).await.map_err(std::io::Error::other)??;
            if addresses.is_empty() || addresses.len()>32 || addresses.iter().any(|address|!public_ip(address.ip())) {return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied).into());}
            Ok(Box::new(addresses.into_iter()) as reqwest::dns::Addrs)
        })
    }
}
pub struct NetworkResponse { pub status:u16,pub content_type:Option<String>,pub location:Option<String>,pub length:Option<u64>,pub stream:ByteStream,pub upload:Option<super::upload::UploadState> }
#[async_trait]
pub trait TransferNetwork:Send+Sync {
    async fn request(&self,url:&Url,method:&str,headers:HeaderMap,body:Upload)->Result<NetworkResponse>;
}
pub struct HttpsNetwork;
#[async_trait]
impl TransferNetwork for HttpsNetwork {
    async fn request(&self,url:&Url,method:&str,headers:HeaderMap,body:Upload)->Result<NetworkResponse> {
        validate_url(url)?;
        let client=reqwest::Client::builder().https_only(true).no_proxy().redirect(reqwest::redirect::Policy::none()).retry(reqwest::retry::never()).dns_resolver(Arc::new(PublicDns)).build().map_err(|_|TransferRefusal::Unavailable)?;
        let upload_state=body.state.clone();
        let mut request=client.request(reqwest::Method::from_bytes(method.as_bytes()).map_err(|_|TransferRefusal::InvalidRequest)?,url.clone()).headers(headers);
        if let Some(content_type)=body.content_type {request=request.header(reqwest::header::CONTENT_TYPE,content_type);}
        request=request.header(reqwest::header::CONTENT_LENGTH,body.length).body(reqwest::Body::wrap_stream(body.stream));
        let response=request.send().await.map_err(|_|upload_state.failure().unwrap_or(TransferRefusal::TransportUnconfirmed))?;
        if let Some(error)=upload_state.failure(){return Err(error);}
        if response.headers().get(reqwest::header::CONTENT_ENCODING).is_some_and(|value|value.as_bytes()!=b"identity") {return Err(TransferRefusal::MimeMismatch);}
        let status=response.status().as_u16();
        let content_type=response.headers().get(reqwest::header::CONTENT_TYPE).filter(|value|value.len()<=256).and_then(|value|value.to_str().ok()).map(|value|value.split(';').next().unwrap_or("").trim().to_ascii_lowercase());
        let location=response.headers().get(reqwest::header::LOCATION).and_then(|value|value.to_str().ok()).filter(|value|value.len()<=8192).map(str::to_owned);
        let length=response.content_length();
        let stream=response.bytes_stream().map(|chunk|match chunk {
            Ok(bytes)=>stream::iter(split_chunk(&bytes)),
            Err(_)=>stream::iter(vec![Err(TransferRefusal::TransportUnconfirmed)]),
        }).flatten().boxed();
        Ok(NetworkResponse {status,content_type,location,length,stream,upload:Some(upload_state)})
    }
}

fn split_chunk(bytes:&[u8])->Vec<Result<Vec<u8>>> {
    if bytes.len()>1024*1024{return vec![Err(TransferRefusal::TooLarge)];}
    bytes.chunks(CHUNK).map(|bytes|Ok(bytes.to_vec())).collect()
}

use crate::sources::{DhcpHostSource, DhcpSourceResult};
use serde::{Serialize, Deserializer, Deserialize};
use crate::error::{DhcpResult, DhcpError};
use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};
use tera::Context;
use std::str::FromStr;
use dhcplib::option::{DhcpOption, DhcpOptions};
use serde_yaml::Value;
use reqwest::header::{HeaderName, HeaderValue, HeaderMap};
use reqwest::{Client, Method};
use serde_yaml::from_value as serde_from_value;
use dhcplib::DhcpPacket;
use url::Url;
use std::time::{Duration, SystemTime};
use serde::de::DeserializeOwned;
use std::fmt::{Display, Formatter};
use tokio::task::JoinHandle;
use tokio::process::Command;
use std::process::Stdio;

macro_rules! to_value {
    ($t:ident, $v:tt) => {
        $v.try_into().and_then(|s: DhcpRestMappingItem| serde_from_value(s.data)
                     .map(DhcpOption::$t)
                     .map_err(Into::into))
    }
}

fn template_values<'a>(value: &'a mut serde_yaml::Value, context: &'a Context) -> DhcpResult<&'a mut serde_yaml::Value> {
    match value {
        Value::String(s) => {
            let t = tera::Tera::one_off(s, context, false)?;
            *value = serde_yaml::from_str(&t)?;
        }
        Value::Sequence(v) => {
            for i in v {
                template_values(i, context)?;
            }
        }
        Value::Mapping(v) => {
            for (_, v) in v {
                template_values(v, context)?;
            }
        }
        _ => {}
    }

    Ok(value)
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
enum DhcpRestMappingItemCustomKind {
    #[serde(alias = "str")]
    String,
    Bool,
    Integer,
    None,
}

impl Default for DhcpRestMappingItemCustomKind {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct DhcpRestMappingItem {
    data: Value,
    #[serde(default)]
    required: bool,
}

impl TryFrom<serde_yaml::Value> for DhcpRestMappingItem {
    type Error = DhcpError;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        serde_from_value(value).map_err(Into::into)
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct DhcpRestMappingItemCustom {
    tag: u8,
    #[serde(default)]
    kind: DhcpRestMappingItemCustomKind,
    #[serde(flatten)]
    item: DhcpRestMappingItem,
}

impl TryFrom<serde_yaml::Value> for DhcpRestMappingItemCustom {
    type Error = DhcpError;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        serde_from_value(value).map_err(Into::into)
    }
}

struct DhcpRestSourceHttpCacheItem<T> {
    data: T,
    time: SystemTime,
}

impl<T> DhcpRestSourceHttpCacheItem<T> {
    fn expired(&self, duration: Duration) -> bool {
        SystemTime::now() > self.time + duration
    }
}

impl From<serde_json::Value> for DhcpRestSourceHttpCacheItem<serde_json::Value> {
    fn from(data: serde_json::Value) -> Self {
        Self {
            data,
            time: SystemTime::now(),
        }
    }
}

#[derive(PartialEq, Eq, Hash)]
struct DhcpRestSourceHttpCacheKey {
    url: Url,
    method: Method,
}

impl Display for DhcpRestSourceHttpCacheKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} - {}", self.method.as_str(), self.url.as_str())
    }
}

struct DhcpRestSourceHttp {
    cache: HashMap<DhcpRestSourceHttpCacheKey, DhcpRestSourceHttpCacheItem<serde_json::Value>>,
    expiration: Duration,
    http: Client,
}

impl DhcpRestSourceHttp {
    fn deserialize_with<'de, D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
    {
        let expiration: f32 = Deserialize::deserialize(deserializer)?;

        Ok(Self {
            cache: Default::default(),
            expiration: Duration::from_secs_f32(expiration),
            http: Default::default(),
        })
    }

    async fn json<T: DeserializeOwned>(&mut self, method: Method, url: Url, body: &Value) -> DhcpResult<T> {
        let key = DhcpRestSourceHttpCacheKey { url: url.clone(), method: method.clone() };

        if let Some(j) = self.cache.get(&key) {
            if !j.expired(self.expiration) { // use cached value
                log::debug!("use cached item {}", key);
                let value = j.data.clone();
                return serde_json::from_value(value).map_err(DhcpError::SerdeJsonError);
            }
            log::debug!("cached item {} expired", key);
            self.cache.remove(&key); // invalidate expired data
        }

        // new request/response
        let request = self.http.request(method, url).json(body).build()?;
        let response = self.http.execute(request).await?;
        let value: serde_json::Value = response.json().await?;
        if self.expiration.as_secs_f32() > 0.0 {
            self.cache.insert(key, value.clone().into());
        }
        serde_json::from_value(value).map_err(DhcpError::SerdeJsonError)
    }
}

impl Default for DhcpRestSourceHttp {
    fn default() -> Self {
        Self {
            cache: Default::default(),
            expiration: Default::default(),
            http: Default::default(),
        }
    }
}

#[derive(Deserialize)]
struct DhcpRestConfigSchemaQuery {
    url: String,
    name: String,
    #[serde(default = "DhcpRestConfigSchemaQuery::ssl_verify")]
    ssl_verify: bool,
    headers: Option<HashMap<String, String>>,
    #[serde(deserialize_with = "DhcpRestSourceHttp::deserialize_with", default)]
    cache: DhcpRestSourceHttp,
    #[serde(deserialize_with = "DhcpRestConfigSchemaQuery::deserialize_with")]
    method: Method,
    #[serde(default)]
    body: Value,
}

impl DhcpRestConfigSchemaQuery {
    fn ssl_verify() -> bool { true }

    fn init(&mut self) -> DhcpResult<()> {
        self.cache.http = Client::builder()
            .danger_accept_invalid_certs(self.ssl_verify)
            .default_headers(Self::map_to_headers(self.headers.as_ref().unwrap_or(&HashMap::new()))?)
            .build()?;
        Ok(())
    }

    fn deserialize_with<'de, D>(deserializer: D) -> Result<Method, D::Error>
        where
            D: Deserializer<'de>,
    {
        let m: String = Deserialize::deserialize(deserializer)?;

        Ok(match m.to_lowercase().as_str() {
            "get" => Method::GET,
            "post" => Method::POST,
            "put" => Method::PUT,
            "patch" => Method::PATCH,
            "delete" => Method::DELETE,
            _ => return Err(serde::de::Error::custom("invalid http method"))
        })
    }

    fn map_to_headers(map: &HashMap<String, String>) -> DhcpResult<HeaderMap> {
        let mut h = HeaderMap::new();

        for (k, v) in map {
            h.insert(HeaderName::from_str(k)?, HeaderValue::from_str(v)?);
        }

        h.insert("Accept", "application/json".parse()?);

        Ok(h)
    }
}

#[derive(Deserialize)]
struct DhcpRestConfigSchemaScript {
    exec: String,
    args: Vec<String>,
    wait: bool,
    #[serde(default = "DhcpRestConfigSchemaScript::timeout")]
    timeout: u64,
}

impl DhcpRestConfigSchemaScript {
    fn timeout() -> u64 { 60 }

    async fn run(&self, context: &Context) -> DhcpResult<()> {
        let program = tera::Tera::one_off(&self.exec, context, false)?;
        let args = self.args.iter().map(|a| {
            tera::Tera::one_off(a, context, false).map_err(Into::into)
        }).collect::<DhcpResult<Vec<String>>>()?;

        log::debug!("running script: {} {}", program, args.join(" "));

        let mut c = Command::new(&program);
        c.args(args);
        c.stdout(Stdio::piped()).stderr(Stdio::piped());

        let child = match c.spawn() {
            Ok(child) => child,
            Err(e) => {
                log::error!("{}: {}", program, e.to_string());
                return Err(e.into());
            }
        };

        let timeout = self.timeout;
        let j: JoinHandle<DhcpResult<()>> = tokio::spawn(async move {
            match tokio::time::timeout(Duration::from_secs(timeout), child.wait_with_output()).await {
                Ok(Ok(output)) => {
                    let stdout = String::from_utf8(output.stdout)
                        .map(|e| format!("stdout: {}", e))
                        .unwrap_or_else(|_| "stdout not utf8".to_string());
                    if output.status.success() {
                        log::info!("{} run successfully, {}", program, stdout)
                    } else {
                        let stderr = String::from_utf8(output.stderr)
                            .map(|e| format!("stderr: {}", e))
                            .unwrap_or_else(|_| "stderr not utf8".to_string());
                        log::error!("{} failed, {}, {}", program, stdout, stderr)
                    }
                }
                Ok(Err(e)) => log::error!("{}: {}", program, e.to_string()),
                Err(_) => log::error!("{} timed out", program),
            }
            Ok(())
        });

        if self.wait {
            j.await??;
        }

        Ok(())
    }
}

#[derive(Deserialize)]
struct DhcpRestSourceConfigSchema {
    scripts: Vec<DhcpRestConfigSchemaScript>,
    queries: Vec<DhcpRestConfigSchemaQuery>,
    mapping: HashMap<String, serde_yaml::Value>,
}

impl DhcpRestSourceConfigSchema {
    fn is_required(value: &Value) -> bool {
        if let serde_yaml::Value::Mapping(m) = value {
            m.get(&serde_yaml::Value::String("required".to_string()))
                .unwrap_or(&serde_yaml::Value::Bool(false))
                .as_bool()
                .unwrap_or(false)
        } else {
            false
        }
    }

    fn context_to_result(&mut self, context: &Context) -> DhcpResult<DhcpSourceResult> {
        let mut client_ip_address = None;
        let mut options = DhcpOptions::new();

        for (key, value) in &mut self.mapping {
            let required = Self::is_required(value);
            let template_result = template_values(value, &context);

            // handle error if required
            match template_result {
                Ok(_) => {}
                Err(e) if required => return Err(e),
                Err(e) => {
                    log::warn!("option templating error {} ({})", key, e);
                    continue;
                }
            }

            let v = value.clone();

            // map + deserialize option
            let option = match key.as_str() {
                "client_ip_address" => {
                    client_ip_address = Some(serde_from_value(v).map_err(|e| {
                        log::error!("{}:{:?} - {}", key, value, e);
                        e
                    })?);
                    continue;
                }
                "subnet_mask" => to_value!(SubnetMask, v),
                "time_offset" => to_value!(TimeOffset, v),
                "router" => to_value!(Router,v),
                "time_server" => to_value!(TimeServer,v),
                "name_server" => to_value!(NameServer,v),
                "domain_name_server" => to_value!(DomainNameServer,v),
                "log_server" => to_value!(LogServer,v),
                "cookie_server" => to_value!(CookieServer,v),
                "lpr_server" => to_value!(LPRServer,v),
                "impress_server" => to_value!(ImpressServer,v),
                "resource_location_server" => to_value!(ResourceLocationServer,v),
                "host_name" => to_value!(HostName,v),
                "boot_file_size" => to_value!(BootFileSize,v),
                "merit_dump_file" => to_value!(MeritDumpFile,v),
                "domain_name" => to_value!(DomainName,v),
                "swap_server" => to_value!(SwapServer,v),
                "root_path" => to_value!(RootPath,v),
                "extension_path" => to_value!(ExtensionPath,v),
                "ip_forwarding" => to_value!(IpForwarding,v),
                "non_local_source_routing" => to_value!(NonLocalSourceRouting,v),
                "policy_filter" => to_value!(PolicyFilter,v),
                "maximum_datagram_reassembly_size" => to_value!(MaximumDatagramReassemblySize,v),
                "default_ip_ttl" => to_value!(DefaultIpTTL,v),
                "path_mtu_aging_timeout" => to_value!(PathMtuAgingTimeout,v),
                "path_mtu_plateau_table" => to_value!(PathMtuPlateauTable,v),
                "interface_mtu" => to_value!(InterfaceMtu,v),
                "all_subnets_local" => to_value!(AllSubnetsLocal,v),
                "broadcast_address" => to_value!(BroadcastAddress,v),
                "mask_supplier" => to_value!(MaskSupplier,v),
                "perform_router_discovery" => to_value!(PerformRouterDiscovery,v),
                "router_solicitation_address" => to_value!(RouterSolicitationAddress,v),
                "static_route" => to_value!(StaticRoute,v),
                "trailer_encapsulation" => to_value!(TrailerEncapsulation,v),
                "arp_cache_timeout" => to_value!(ArpCacheTimeout,v),
                "ethernet_encapsulation" => to_value!(EthernetEncapsulation,v),
                "tcp_default_ttl" => to_value!(TcpDefaultTTL,v),
                "tcp_keep_alive_interval" => to_value!(TcpKeepAliveInterval,v),
                "tcp_keep_alive_garbage" => to_value!(TcpKeepAliveGarbage,v),
                "network_information_service_domain" => to_value!(NetworkInformationServiceDomain,v),
                "network_information_servers" => to_value!(NetworkInformationServers,v),
                "network_time_protocol_servers" => to_value!(NetworkTimeProtocolServers,v),
                "vendor_specific" => to_value!(VendorSpecific,v),
                "net_bios_over_tcp_ip_name_server" => to_value!(NetBiosOverTcpIpNameServer,v),
                "net_bios_over_tcp_ip_datagram_distribution_server" => to_value!(NetBiosOverTcpIpDatagramDistributionServer,v),
                "net_bios_over_tcp_ip_node_type" => to_value!(NetBiosOverTcpIpNodeType,v),
                "net_bios_over_tcp_ip_scope" => to_value!(NetBiosOverTcpIpScope,v),
                "x_window_system_font_server" => to_value!(XWindowSystemFontServer,v),
                "x_window_system_display_manager" => to_value!(XWindowSystemDisplayManager,v),
                "requested_ip_address" => to_value!(RequestedIpAddress,v),
                "ip_address_lease_time" => to_value!(IpAddressLeaseTime,v),
                "option_overload" => to_value!(OptionOverload,v),
                "message_type" => to_value!(MessageType,v),
                "server_identifier" => to_value!(ServerIdentifier,v),
                "parameter_request_list" => to_value!(ParameterRequestList,v),
                "message" => to_value!(Message,v),
                "maximum_dhcp_message_size" => to_value!(MaximumDhcpMessageSize,v),
                "renewal_time_value" => to_value!(RenewalTimeValue,v),
                "rebinding_time_value" => to_value!(RebindingTimeValue,v),
                "vendor_class_identifier" => to_value!(VendorClassIdentifier,v),
                "client_identifier" => to_value!(ClientIdentifier,v),
                "network_information_service_plus_domain" => to_value!(NetworkInformationServicePlusDomain,v),
                "network_information_service_plus_server" => to_value!(NetworkInformationServicePlusServer,v),
                "tftp_server" => to_value!(TftpServer,v),
                "boot_file_name" => to_value!(BootFileName,v),
                "mobile_ip_home_agent" => to_value!(MobileIpHomeAgent,v),
                "smtp_server" => to_value!(SmtpServer,v),
                "pop3_server" => to_value!(Pop3Server,v),
                "nntp_server" => to_value!(NntpServer,v),
                "www_server" => to_value!(WwwServer,v),
                "finger_server" => to_value!(FingerServer,v),
                "irc_server" => to_value!(IrcServer,v),
                "street_talk_server" => to_value!(StreetTalkServer,v),
                "street_talk_directory_assistance_server" => to_value!(StreetTalkDirectoryAssistanceServer,v),
                _ => DhcpRestMappingItemCustom::try_from(v).and_then(TryInto::try_into) // custom options
            };

            // handle errors if required
            match option {
                Ok(v) => options.upsert(v),
                Err(e) if required => return Err(e),
                Err(e) => log::warn!("invalid option mapping: {}:{:?} ({})", key, value, e)
            }
        }

        Ok(DhcpSourceResult::new(client_ip_address, options))
    }
}

#[derive(Deserialize)]
struct DhcpRestSourceConfig {
    offer: DhcpRestSourceConfigSchema,
    reserve: DhcpRestSourceConfigSchema,
    release: DhcpRestSourceConfigSchema,
    decline: DhcpRestSourceConfigSchema,
    inform: DhcpRestSourceConfigSchema,
}

pub(crate) struct DhcpRestSource {
    config: DhcpRestSourceConfig
}

impl DhcpRestSource {
    async fn query(config: &mut DhcpRestSourceConfigSchema, p: &DhcpPacket) -> DhcpResult<Context> {
        let mut context = Context::new();

        context.insert("client_hardware_address", &p.client_hardware().to_string());
        context.insert("client_ip_address", &p.client());
        context.insert("server_ip_address", &p.server());
        context.insert("client_hostname", &p.hostname());

        let mut queries: HashMap<String, serde_json::Value> = HashMap::new();
        for q in &mut config.queries {
            let templated_query = tera::Tera::one_off(&q.url, &context, false)?;
            template_values(&mut q.body, &context)?;
            let result: serde_json::Value = q.cache.json(q.method.clone(), templated_query.parse()?, &q.body).await?;

            queries.insert(q.name.clone(), result);
            context.insert("results", &queries)
        }

        Ok(context)
    }
}

impl TryInto<DhcpOption> for DhcpRestMappingItemCustom {
    type Error = DhcpError;

    fn try_into(self) -> Result<DhcpOption, Self::Error> {
        let d = match self.item.data {
            Value::Null => vec![],
            Value::Bool(v) => {
                if v { vec![1] } else { vec![0] }
            }
            Value::Number(v) => {
                if let Some(i) = v.as_i64() {
                    i.to_be_bytes().to_vec()
                } else if let Some(i) = v.as_f64() {
                    i.to_be_bytes().to_vec()
                } else if let Some(i) = v.as_u64() {
                    i.to_be_bytes().to_vec()
                } else {
                    return Err(DhcpError::CustomRestTypeError);
                }
            }
            Value::String(v) => v.as_bytes().to_vec(),
            _ => return Err(DhcpError::CustomRestTypeError),
        };
        Ok(DhcpOption::Unknown(self.tag, d))
    }
}

#[async_trait::async_trait]
impl DhcpHostSource for DhcpRestSource {
    const NAME: &'static str = "rest";

    async fn offer(&mut self, p: &DhcpPacket) -> DhcpResult<Option<DhcpSourceResult>> {
        let c = Self::query(&mut self.config.offer, p).await?;

        for script in &self.config.offer.scripts {
            script.run(&c).await?;
        }

        self.config.offer.context_to_result(&c).map(Option::Some)
    }

    async fn reserve(&mut self, p: &DhcpPacket) -> DhcpResult<Option<DhcpSourceResult>> {
        let c = Self::query(&mut self.config.reserve, p).await?;
        self.config.reserve.context_to_result(&c).map(Option::Some)
    }

    async fn release(&mut self, p: &DhcpPacket) -> DhcpResult<()> {
        Self::query(&mut self.config.release, p).await.map(|_| ())
    }

    async fn decline(&mut self, p: &DhcpPacket) -> DhcpResult<()> {
        Self::query(&mut self.config.decline, p).await.map(|_| ())
    }

    async fn inform(&mut self, p: &DhcpPacket) -> DhcpResult<Option<DhcpSourceResult>> {
        let c = Self::query(&mut self.config.inform, p).await?;
        self.config.inform.context_to_result(&c).map(Option::Some)
    }

    fn from_config<'a, T: Deserializer<'a> + Send>(config: T) -> DhcpResult<Self> where Self: Sized {
        let mut s = Self {
            config: Deserialize::deserialize(config).map_err(|e| DhcpError::SerdeErrorString(e.to_string()))?
        };

        // init cache clients
        for queries in [
            &mut s.config.decline.queries,
            &mut s.config.release.queries,
            &mut s.config.inform.queries,
            &mut s.config.reserve.queries,
            &mut s.config.offer.queries
        ] {
            for query in queries.iter_mut() {
                query.init()?;
            }
        }

        Ok(s)
    }
}

#[tokio::test]
async fn test() {
    let url = &mockito::server_url();
    let body = serde_json::json!({
            "test": "data"
        });
    let _m = mockito::mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .match_header("token", "12345")
        .match_body(body.to_string().as_str())
        .with_body(serde_json::json!({
            "device1": {
                "ip": "1.2.3.4",
                "mask": "255.255.255.0",
                "router": ["1.1.1.1", "2.2.2.2"]
            },
            "device2": {
                "ip": "5.5.5.5",
                "mask": "255.255.255.0",
                "custom_data": "something",
            }
        }).to_string())
        .create();

    let mut m = HashMap::new();

    m.insert("client_ip_address".to_string(), Value::from("{{ results.test.device1.ip }}"));
    m.insert("subnet_mask".to_string(), serde_yaml::to_value(DhcpRestMappingItem {
        data: Value::from("{{ results.test.device1.mask }}"),
        required: false,
    }).unwrap());
    m.insert("router".to_string(), serde_yaml::to_value(DhcpRestMappingItem {
        data: Value::from("{{ results.test.device1.router }}"),
        required: false,
    }).unwrap());
    m.insert("custom1".to_string(), serde_yaml::to_value(DhcpRestMappingItemCustom {
        tag: 200,
        kind: DhcpRestMappingItemCustomKind::Integer,
        item: DhcpRestMappingItem { data: Value::from("1234567890"), required: false },
    }).unwrap());

    let mut headers = HashMap::new();
    headers.insert("token".to_string(), "12345".to_string());

    let mut query = DhcpRestConfigSchemaQuery {
        url: format!("{}/", url),
        name: "test".to_string(),
        ssl_verify: false,
        headers: Some(headers),
        cache: Default::default(),
        method: Method::POST,
        body: serde_yaml::to_value(body).unwrap(),
    };

    query.init().unwrap();
    let s = DhcpRestSourceConfigSchema {
        scripts: vec![],
        queries: vec![query],
        mapping: m,
    };

    let mut s = DhcpRestSource {
        config: DhcpRestSourceConfig {
            offer: s,
            reserve: DhcpRestSourceConfigSchema {
                scripts: vec![],
                queries: vec![],
                mapping: Default::default(),
            },
            release: DhcpRestSourceConfigSchema {
                scripts: vec![],
                queries: vec![],
                mapping: Default::default(),
            },
            decline: DhcpRestSourceConfigSchema {
                scripts: vec![],
                queries: vec![],
                mapping: Default::default(),
            },
            inform: DhcpRestSourceConfigSchema {
                scripts: vec![],
                queries: vec![],
                mapping: Default::default(),
            },
        }
    };

    let result = s.offer(&DhcpPacket::new(
        dhcplib::MessageOperation::BootRequest,
        dhcplib::HardwareAddressType::Ethernet,
        0,
        123,
        0,
        dhcplib::Flags::Broadcast,
        std::net::Ipv4Addr::UNSPECIFIED,
        std::net::Ipv4Addr::UNSPECIFIED,
        std::net::Ipv4Addr::UNSPECIFIED,
        std::net::Ipv4Addr::UNSPECIFIED,
        macaddr::MacAddr6::new(1, 2, 3, 5, 6, 7),
        ascii::AsciiString::new(),
        ascii::AsciiString::new(),
        dhcplib::option::DhcpOptions::new_with_options(vec![
            dhcplib::option::DhcpOption::SubnetMask(std::net::Ipv4Addr::new(1, 2, 3, 4))
        ]),
    )).await.unwrap().unwrap();

    assert_eq!(result.client_ip_address, Some(std::net::Ipv4Addr::new(1, 2, 3, 4)));
    assert_eq!(result.options.try_ipv4_option(dhcplib::option::SUBNET_MASK).unwrap(), std::net::Ipv4Addr::new(255, 255, 255, 0));
    assert_eq!(result.options.try_ipv4vec_option(dhcplib::option::ROUTER).unwrap(), vec![std::net::Ipv4Addr::new(1, 1, 1, 1), std::net::Ipv4Addr::new(2, 2, 2, 2)]);
    assert_eq!(result.options.option(200).unwrap(), &dhcplib::option::DhcpOption::Unknown(200, vec![0, 0, 0, 0, 73, 150, 2, 210]));
}

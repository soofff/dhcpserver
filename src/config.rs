use serde::{Serialize, Deserialize};
use std::path::Path;
use std::fs::File;
use crate::error::{DhcpResult, DhcpError};
use crate::sources::DhcpHostSource;
use crate::sources::rest::DhcpRestSource;
use std::net::Ipv4Addr;
use structopt::StructOpt;
use simplelog::LevelFilter;

#[derive(Serialize, Deserialize)]
struct Sources {
    kind: String,
    config: serde_yaml::Value,
}

#[derive(Serialize, Deserialize)]
pub struct DhcpConfig {
    #[serde(default = "DhcpConfig::default_port")]
    port: u16,
    listen: Option<Vec<Ipv4Addr>>,
    sources: Vec<Sources>,
}

impl DhcpConfig {
    fn default_port() -> u16 {
        67
    }

    pub fn port(&self) -> u16 { self.port }

    pub fn ips(&self) -> Option<&Vec<Ipv4Addr>> {
        self.listen.as_ref()
    }

    pub fn from_file<P: AsRef<Path>>(path: P) -> DhcpResult<Self> {
        let file = File::open(path)?;
        serde_yaml::from_reader(file).map_err(Into::into)
    }

    pub fn init_sources(&mut self) -> DhcpResult<Vec<impl DhcpHostSource>> {
        let mut sources = vec![];

        for source in self.sources.drain(..) { // clone sources?
            match source.kind.as_str() {
                DhcpRestSource::NAME => sources.push(DhcpRestSource::from_config(source.config)?),
                _ => return Err(DhcpError::SourceKindUnknown)
            }
        }

        Ok(sources)
    }
}

#[derive(Debug, StructOpt)]
pub struct DhcpConfigOptions {
    #[structopt(short, long, env = "DHCP_CONFIG", help = "default path: ./config.y[a]ml")]
    config: Option<String>,

    #[structopt(short, long, default_value="info", env = "DHCP_VERBOSITY", help = "off, error, warn, info, debug trace")]
    verbosity: LevelFilter
}

impl DhcpConfigOptions {
    pub fn config(&self) -> Option<&str> {
        if let Some(s) = &self.config {
            return Some(&s);
        } else {
            for s in ["./config.yml", "./config.yaml"] {
                let p = Path::new(s);
                if p.exists() {
                    return Some(s);
                }
            }
        }
        None
    }

    pub fn verbosity(&self) -> LevelFilter { self.verbosity }
}

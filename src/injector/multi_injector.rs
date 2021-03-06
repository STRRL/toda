use super::attr_override_injector::AttrOverrideInjector;
use super::fault_injector::FaultInjector;
use super::filter;
use super::injector_config::InjectorConfig;
use super::latency_injector::LatencyInjector;
use super::Injector;
use crate::hookfs::{Reply, Result};

use async_trait::async_trait;
use fuser::FileAttr;
use log::trace;

use std::path::Path;

#[derive(Debug)]
pub struct MultiInjector {
    injectors: Vec<Box<dyn Injector>>,
}

impl MultiInjector {
    pub fn build(conf: Vec<InjectorConfig>) -> anyhow::Result<Self> {
        trace!("build multiinjectors");
        let mut injectors = Vec::new();

        for injector in conf.into_iter() {
            let injector = match injector {
                InjectorConfig::Fault(faults) => {
                    (box FaultInjector::build(faults)?) as Box<dyn Injector>
                }
                InjectorConfig::Latency(latency) => {
                    (box LatencyInjector::build(latency)?) as Box<dyn Injector>
                }
                InjectorConfig::AttrOverride(attr_override) => {
                    (box AttrOverrideInjector::build(attr_override)?) as Box<dyn Injector>
                }
            };
            injectors.push(injector)
        }

        Ok(Self { injectors })
    }
}

#[async_trait]
impl Injector for MultiInjector {
    async fn inject(&self, method: &filter::Method, path: &Path) -> Result<()> {
        for injector in self.injectors.iter() {
            injector.inject(method, path).await?
        }

        Ok(())
    }

    fn inject_reply(&self, method: &filter::Method, path: &Path, reply: &mut Reply) -> Result<()> {
        for injector in self.injectors.iter() {
            injector.inject_reply(method, path, reply)?
        }

        Ok(())
    }

    fn inject_attr(&self, attr: &mut FileAttr, path: &Path) {
        for injector in self.injectors.iter() {
            injector.inject_attr(attr, path)
        }
    }
}

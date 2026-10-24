//! Bounded document resources. Local patch for long-lived virtualized views.
use std::collections::HashMap;

use crate::{BaseDocument, NodeId, node::ImageData, util::ImageType};
#[cfg(test)]
#[path = "image_observation_tests.rs"]
mod image_observation_tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceLimits {
    pub requests: usize,
    pub completion_bytes: usize,
    pub encoded_bytes: usize,
    pub image_cache_entries: usize,
    pub image_cache_bytes: usize,
    pub image_decoded_bytes: u64,
    pub image_dimension: u32,
    pub pending_images: usize,
    pub image_waiters: usize,
    pub font_entries: usize,
    pub font_bytes: usize,
}
impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            requests: 256,
            completion_bytes: 32 * 1024 * 1024,
            encoded_bytes: 8 * 1024 * 1024,
            image_cache_entries: 64,
            image_cache_bytes: 16 * 1024 * 1024,
            image_decoded_bytes: 16 * 1024 * 1024,
            image_dimension: 4096,
            pending_images: 128,
            image_waiters: 4096,
            font_entries: 64,
            font_bytes: 32 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResourceStats {
    pub requests: usize,
    pub completions: usize,
    pub completion_bytes: usize,
    pub decode_waiters: usize,
    pub cached_images: usize,
    pub cached_image_bytes: usize,
    pub image_evictions: u64,
    pub pending_images: usize,
    pub image_waiters: usize,
    pub registered_fonts: usize,
    pub registered_font_bytes: usize,
    pub rejected: u64,
    pub failures: u64,
}

/// One live element consumer, independently of a shared URL fetch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageRequestId(pub u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageLoadStatus {
    Loaded,
    Failed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageLoadEvent {
    pub node: NodeId,
    pub request: ImageRequestId,
    pub status: ImageLoadStatus,
}
#[derive(Default)]
pub(crate) struct ImageObservations {
    next: u64,
    live: HashMap<NodeId, ImageConsumer>,
    sequence: u64,
    ready: HashMap<NodeId, (u64, ImageLoadEvent)>,
}
struct ImageConsumer {
    request: ImageRequestId,
    terminal: bool,
}
impl ImageObservations {
    pub fn begin(&mut self, node: NodeId) -> Option<ImageRequestId> {
        self.ready.remove(&node);
        self.live.remove(&node);
        let next = self.next.checked_add(1)?;
        self.next = next;
        let request = ImageRequestId(next);
        self.live.insert(
            node,
            ImageConsumer {
                request,
                terminal: false,
            },
        );
        Some(request)
    }
    pub fn retire(&mut self, node: NodeId) {
        self.live.remove(&node);
        self.ready.remove(&node);
    }
    pub fn complete(&mut self, node: NodeId, request: ImageRequestId, status: ImageLoadStatus) {
        if let Some(consumer) = self.live.get_mut(&node)
            && consumer.request == request
            && !consumer.terminal
        {
            consumer.terminal = true;
            self.sequence = self
                .sequence
                .checked_add(1)
                .expect("image completion count is bounded by admitted requests");
            self.ready.insert(
                node,
                (
                    self.sequence,
                    ImageLoadEvent {
                        node,
                        request,
                        status,
                    },
                ),
            );
        }
    }
}

pub(crate) struct PendingImage {
    pub request_id: usize,
    pub waiters: Vec<(NodeId, ImageType, Option<ImageRequestId>)>,
}

#[derive(Default)]
pub(crate) struct ImageCache {
    entries: HashMap<String, (ImageData, usize, u128)>,
    bytes: usize,
    clock: u128,
    evictions: u64,
    limits: ResourceLimits,
}
impl ImageCache {
    pub fn get(&mut self, url: &str) -> Option<&ImageData> {
        self.clock = self.clock.saturating_add(1);
        let entry = self.entries.get_mut(url)?;
        entry.2 = self.clock;
        Some(&entry.0)
    }
    pub fn insert(&mut self, url: String, image: ImageData) {
        // SVG owns an opaque tree: do not pretend its allocation is RGBA size.
        // Live elements can still hold it; the optional reuse cache skips it.
        let ImageData::Raster(raster) = &image else {
            return;
        };
        let Some(bytes) = raster.data.len().checked_add(url.len()) else {
            return;
        };
        if let Some(old) = self.entries.remove(&url) {
            self.bytes -= old.1;
        }
        if self.limits.image_cache_entries == 0 || bytes > self.limits.image_cache_bytes {
            return;
        }
        while self.entries.len() >= self.limits.image_cache_entries
            || self.bytes > self.limits.image_cache_bytes - bytes
        {
            self.evict();
        }
        self.clock = self.clock.saturating_add(1);
        self.bytes += bytes;
        self.entries.insert(url, (image, bytes, self.clock));
    }
    fn evict(&mut self) {
        let Some(key) = self
            .entries
            .iter()
            .min_by_key(|(_, value)| value.2)
            .map(|(key, _)| key.clone())
        else {
            return;
        };
        self.bytes -= self.entries.remove(&key).expect("cache entry").1;
        self.evictions = self.evictions.saturating_add(1);
    }
    fn set_limits(&mut self, limits: ResourceLimits) {
        self.limits = limits;
        while self.entries.len() > limits.image_cache_entries
            || self.bytes > limits.image_cache_bytes
        {
            self.evict();
        }
    }
}

impl BaseDocument {
    pub(crate) fn cancel_replaced_image_consumers(&mut self, node: NodeId) {
        let provider = &self.net_provider;
        let document = self.id();
        self.pending_images.retain(|url, pending| {
            pending
                .waiters
                .retain(|(id, _, request)| *id != node || request.is_none());
            if pending.waiters.is_empty() {
                self.tx.pool.cancel(pending.request_id);
                provider.cancel(document, url);
                false
            } else {
                true
            }
        });
    }
    /// Current element-consumer identity; changes even for an explicit same-URL reload.
    pub fn image_request(&self, node: NodeId) -> Option<ImageRequestId> {
        self.image_observations
            .live
            .get(&node)
            .map(|consumer| consumer.request)
    }
    /// Terminal observations recorded after actual resource consumption, including cache hits.
    /// Storage contains at most one result per live image element; no URL/history scan.
    pub fn take_image_events(&mut self) -> Vec<ImageLoadEvent> {
        let mut events: Vec<_> = self
            .image_observations
            .ready
            .drain()
            .map(|(_, event)| event)
            .collect();
        events.sort_by_key(|(sequence, _)| *sequence);
        events.into_iter().map(|(_, event)| event).collect()
    }

    pub fn resource_stats(&self) -> ResourceStats {
        let pool = self.tx.pool.stats();
        ResourceStats {
            requests: pool.0,
            completions: pool.1,
            completion_bytes: pool.2,
            decode_waiters: pool.3,
            cached_images: self.image_cache.entries.len(),
            cached_image_bytes: self.image_cache.bytes,
            image_evictions: self.image_cache.evictions,
            pending_images: self.pending_images.len(),
            image_waiters: self
                .pending_images
                .values()
                .map(|image| image.waiters.len())
                .sum(),
            registered_fonts: self.resource_counters.registered_fonts,
            registered_font_bytes: self.resource_counters.registered_font_bytes,
            rejected: self.resource_counters.rejected.saturating_add(pool.4),
            failures: self.resource_counters.failures,
        }
    }
    pub fn set_resource_limits(&mut self, limits: ResourceLimits) -> Result<(), &'static str> {
        if self.resource_counters.registered_fonts > limits.font_entries
            || self.resource_counters.registered_font_bytes > limits.font_bytes
            || self.pending_images.len() > limits.pending_images
            || self
                .pending_images
                .values()
                .any(|pending| pending.waiters.len() > limits.image_waiters)
        {
            return Err("new resource limits are below live non-evictable resources");
        }
        self.tx.pool.set_limits(limits)?;
        self.resource_limits = limits;
        self.image_cache.set_limits(limits);
        Ok(())
    }
    /// Cancel image requests without a live consumer. Work depends only on the
    /// bounded pending table, never the source data or all visited URLs.
    pub fn prune_resource_interests(&mut self) {
        let nodes = &self.nodes;
        let base = &self.url;
        let provider = &self.net_provider;
        let doc_id = self.id();
        let mut removed = Vec::new();
        self.pending_images.retain(|url, pending| {
            pending.waiters.retain(|(id, kind, request)| {
                if request.is_some()
                    && self
                        .image_observations
                        .live
                        .get(id)
                        .map(|consumer| consumer.request)
                        != *request
                {
                    return false;
                }
                let Some(node) = nodes.get(*id).filter(|node| node.flags.is_in_document()) else {
                    removed.push((*id, *kind, url.clone()));
                    return false;
                };
                let Some(element) = node.element_data() else {
                    return false;
                };
                match kind {
                    ImageType::Image => element
                        .attr(crate::local_name!("src"))
                        .and_then(|value| base.resolve_relative(value))
                        .is_some_and(|value| value.as_str() == url),
                    ImageType::Background(index) => element
                        .background_images
                        .get(*index)
                        .and_then(|value| value.as_ref())
                        .is_some_and(|value| value.url.as_str() == url),
                    ImageType::Mask(index) => element
                        .mask_images
                        .get(*index)
                        .and_then(|value| value.as_ref())
                        .is_some_and(|value| value.url.as_str() == url),
                }
            });
            if pending.waiters.is_empty() {
                self.tx.pool.cancel(pending.request_id);
                provider.cancel(doc_id, url);
                false
            } else {
                true
            }
        });
        for (id, kind, url) in removed {
            let Some(element) = self
                .nodes
                .get_mut(id)
                .and_then(|node| node.element_data_mut())
            else {
                continue;
            };
            let layer = match kind {
                ImageType::Background(index) => element.background_images.get_mut(index),
                ImageType::Mask(index) => element.mask_images.get_mut(index),
                ImageType::Image => None,
            };
            if let Some(Some(layer)) = layer
                && layer.url.as_str() == url
            {
                layer.status = crate::node::Status::Error;
            }
        }
    }
}

impl Drop for BaseDocument {
    fn drop(&mut self) {
        self.tx.pool.close();
        drop(self.rx.take());
        self.net_provider.close(self.id());
    }
}

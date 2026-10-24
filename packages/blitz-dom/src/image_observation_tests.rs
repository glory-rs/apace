use crate::net::{Resource, ResourceLoadResponse};
use crate::util::ImageType;
use crate::{Attribute, BaseDocument, DocumentConfig, ImageLoadStatus, NodeId, qual_name};
use blitz_traits::net::{NetHandler, NetProvider, Request};
use std::sync::{Arc, Mutex};
#[derive(Default)]
struct Provider(Mutex<Vec<Box<dyn NetHandler>>>);
impl NetProvider for Provider {
    fn fetch(&self, _: usize, _: Request, handler: Box<dyn NetHandler>) {
        self.0.lock().unwrap().push(handler);
    }
}
fn fixture() -> (BaseDocument, NodeId, Arc<Provider>) {
    let provider = Arc::new(Provider::default());
    let mut doc = BaseDocument::new(DocumentConfig {
        net_provider: Some(provider.clone()),
        base_url: Some("https://example.test/".into()),
        ..Default::default()
    });
    let root = doc.root_node().id;
    let mut m = doc.mutate();
    let html = m.create_element(qual_name!("html", html), vec![]);
    let body = m.create_element(qual_name!("body", html), vec![]);
    m.append_children(root, &[html]);
    m.append_children(html, &[body]);
    drop(m);
    (doc, body, provider)
}
fn mount(doc: &mut BaseDocument, body: NodeId, source: &str) -> NodeId {
    let mut m = doc.mutate();
    let image = m.create_element(
        qual_name!("img", html),
        vec![Attribute {
            name: qual_name!("src"),
            value: source.into(),
        }],
    );
    m.append_children(body, &[image]);
    image
}
fn response(request: usize, success: bool) -> ResourceLoadResponse {
    ResourceLoadResponse {
        request_id: request,
        node_id: None,
        resolved_url: None,
        result: if success {
            Ok(Resource::Image(
                ImageType::Image,
                1,
                1,
                Arc::new(vec![255, 0, 0, 255]),
            ))
        } else {
            Err("decode or transport failed".into())
        },
        ticket: None,
    }
}
fn request(doc: &BaseDocument, source: &str) -> usize {
    doc.pending_images[&format!("https://example.test/{source}")].request_id
}
#[test]
fn image_observation_waits_for_consumption_and_cache_hits_have_a_new_consumer() {
    let (mut doc, body, _provider) = fixture();
    let image = mount(&mut doc, body, "a.png");
    let first = doc.image_request(image).unwrap();
    assert!(doc.take_image_events().is_empty());
    let request = request(&doc, "a.png");
    doc.load_resource(response(request, true));
    let events = doc.take_image_events();
    assert_eq!(events.len(), 1);
    assert_eq!(
        (events[0].node, events[0].request, events[0].status),
        (image, first, ImageLoadStatus::Loaded)
    );
    assert!(
        doc.get_node(image)
            .unwrap()
            .element_data()
            .unwrap()
            .raster_image_data()
            .is_some()
    );
    doc.mutate()
        .set_attribute(image, qual_name!("src"), "a.png");
    let retry = doc.image_request(image).unwrap();
    assert_ne!(retry, first);
    let events = doc.take_image_events();
    assert_eq!(events.len(), 1);
    assert_eq!(
        (events[0].request, events[0].status),
        (retry, ImageLoadStatus::Loaded)
    );
}
#[test]
fn image_observation_aba_rejects_obsolete_waiters_and_old_response_ids() {
    let (mut doc, body, _provider) = fixture();
    let image = mount(&mut doc, body, "a.png");
    let first = doc.image_request(image).unwrap();
    let old_a = request(&doc, "a.png");
    doc.mutate()
        .set_attribute(image, qual_name!("src"), "b.png");
    let old_b = request(&doc, "b.png");
    doc.mutate()
        .set_attribute(image, qual_name!("src"), "a.png");
    let current = doc.image_request(image).unwrap();
    let new_a = request(&doc, "a.png");
    assert_ne!(first, current);
    assert_ne!(old_a, new_a);
    doc.load_resource(response(old_a, false));
    doc.load_resource(response(old_b, true));
    assert!(doc.take_image_events().is_empty());
    doc.load_resource(response(new_a, true));
    let events = doc.take_image_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].request, current);
}
#[test]
fn image_observation_shared_fetch_keeps_independent_consumers_and_fifo_completion_order() {
    let (mut doc, body, _provider) = fixture();
    let first = mount(&mut doc, body, "a.png");
    let second = mount(&mut doc, body, "a.png");
    let third = mount(&mut doc, body, "b.png");
    assert_eq!(doc.pending_images.len(), 2);
    let a = request(&doc, "a.png");
    let b = request(&doc, "b.png");
    doc.load_resource(response(b, false));
    doc.load_resource(response(a, true));
    let events = doc.take_image_events();
    assert_eq!(
        events.iter().map(|event| event.node).collect::<Vec<_>>(),
        [third, first, second]
    );
    assert_ne!(events[1].request, events[2].request);
}
#[test]
fn image_observation_replacement_disposal_and_reused_slots_do_not_retain_history() {
    let (mut doc, body, _provider) = fixture();
    let image = mount(&mut doc, body, "a.png");
    let old = request(&doc, "a.png");
    doc.load_resource(response(old, true));
    doc.mutate().remove_and_drop_node(image);
    assert!(doc.take_image_events().is_empty());
    assert!(doc.image_request(image).is_none());
    let replacement = mount(&mut doc, body, "b.png");
    doc.load_resource(response(old, false));
    assert!(doc.take_image_events().is_empty());
    let current = request(&doc, "b.png");
    doc.load_resource(response(current, true));
    assert_eq!(doc.take_image_events()[0].node, replacement);
    doc.mutate().clear_attribute(replacement, qual_name!("src"));
    assert_eq!(doc.take_image_events()[0].status, ImageLoadStatus::Failed);
    assert_eq!(doc.image_observations.live.len(), 1);
    assert!(doc.image_observations.ready.is_empty());
}
#[test]
fn image_observation_admission_failure_and_no_provider_are_terminal_and_bounded() {
    let (mut doc, body, _provider) = fixture();
    doc.set_resource_limits(crate::ResourceLimits {
        pending_images: 0,
        ..Default::default()
    })
    .unwrap();
    let image = mount(&mut doc, body, "a.png");
    let events = doc.take_image_events();
    assert_eq!(events.len(), 1);
    assert_eq!(
        (events[0].node, events[0].status),
        (image, ImageLoadStatus::Failed)
    );
    doc.net_provider = Arc::new(blitz_traits::net::DummyNetProvider);
    doc.mutate()
        .set_attribute(image, qual_name!("src"), "b.png");
    assert_eq!(doc.take_image_events()[0].status, ImageLoadStatus::Failed);
    doc.image_observations.next = u64::MAX;
    doc.mutate()
        .set_attribute(image, qual_name!("src"), "c.png");
    assert!(doc.image_request(image).is_none());
    assert!(doc.take_image_events().is_empty());
}

#[test]
fn image_observation_one_terminal_per_consumer_survives_draining_and_sequence_is_bounded() {
    let (mut doc, body, _provider) = fixture();
    let image = mount(&mut doc, body, "a.png");
    let consumer = doc.image_request(image).unwrap();
    let request = request(&doc, "a.png");
    doc.load_resource(response(request, true));
    assert_eq!(doc.take_image_events().len(), 1);
    let sequence = doc.image_observations.sequence;
    for _ in 0..4096 {
        doc.image_observations
            .complete(image, consumer, ImageLoadStatus::Failed);
        assert!(doc.take_image_events().is_empty());
    }
    assert_eq!(doc.image_observations.sequence, sequence);
    doc.mutate()
        .set_attribute(image, qual_name!("src"), "a.png");
    assert_eq!(doc.take_image_events().len(), 1);
    assert_eq!(doc.image_observations.sequence, sequence + 1);
    assert!(doc.image_observations.sequence <= doc.image_observations.next);
}

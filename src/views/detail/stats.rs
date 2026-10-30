//! The Stats tab: CPU, memory, network and disk cards with the latest numbers and, for CPU and
//! memory, a chart of the last minute.
//!
//! The stream is followed while the container runs; it ends when the container stops, which
//! only means "stopped", not "gone" (see `App::on_docker_event` for that).

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use futures::{Stream, StreamExt};
use iced::widget::{Column, center, column, container, row, scrollable, text};
use iced::{Element, Length, Subscription};
use rust_i18n::t;

use super::{DetailState, Msg};
use crate::app::Message;
use crate::docker::DockerClient;
use crate::model::StatsSample;
use crate::ui::components::{human_bytes, sparkline};
use crate::ui::fonts::{MONO, SANS_MEDIUM};
use crate::ui::style;
use crate::ui::tokens::{SPACE_1, SPACE_2, SPACE_3, SPACE_4, TEXT_LG, TEXT_SM};

/// The stats messages of one container: a `StatsSample` per reading, then `StatsEnded`. Any
/// error ends the stream the same way: the list reload tells whether the container is gone.
pub fn stats_messages(
    client: Arc<dyn DockerClient>,
    id: String,
    generation: u64,
) -> impl Stream<Item = Message> {
    let sample_id = id.clone();
    client
        .stats(&id)
        .take_while(|raw| std::future::ready(raw.is_ok()))
        .filter_map(|raw| std::future::ready(raw.ok()))
        .map(move |raw| {
            Message::Detail(Msg::StatsSample {
                id: sample_id.clone(),
                generation,
                raw,
            })
        })
        .chain(futures::stream::once(std::future::ready(Message::Detail(
            Msg::StatsEnded { id, generation },
        ))))
}

/// Identity of the stats subscription: another container or generation starts a new stream.
pub struct StatsSource {
    pub client: Arc<dyn DockerClient>,
    pub id: String,
    pub generation: u64,
}

impl Hash for StatsSource {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.generation.hash(state);
    }
}

pub fn subscription(source: StatsSource) -> Subscription<Message> {
    Subscription::run_with(source, |s| {
        stats_messages(s.client.clone(), s.id.clone(), s.generation)
    })
}

/// A titled card: the main number and, if there is one, a chart below it.
fn card<'a>(
    title: String,
    lines: Vec<(Option<String>, String)>,
    chart: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut body: Column<'a, Message> = column![
        text(title)
            .size(TEXT_SM)
            .font(SANS_MEDIUM)
            .style(style::text::muted)
    ]
    .spacing(SPACE_1);
    for (label, value) in lines {
        let value = text(value).size(TEXT_LG).font(MONO);
        body = body.push(match label {
            Some(label) => row![
                text(label)
                    .size(TEXT_SM)
                    .style(style::text::faint)
                    .width(Length::Fill),
                value,
            ]
            .spacing(SPACE_2)
            .into(),
            None => Element::from(value),
        });
    }
    if let Some(chart) = chart {
        body = body.push(chart);
    }
    container(body)
        .padding(SPACE_3)
        .width(Length::Fill)
        .style(style::container::card)
        .into()
}

fn cards<'a>(d: &'a DetailState, last: &StatsSample) -> Element<'a, Message> {
    let cpu = card(
        t!("stats.cpu").into_owned(),
        vec![(None, format!("{:.1}%", last.cpu_percent))],
        Some(sparkline(
            &d.stats.cpu_series(),
            Some(100.0 * f64::from(d.cpus)),
        )),
    );
    let memory = card(
        t!("stats.memory").into_owned(),
        vec![(
            None,
            if last.mem_limit > 0 {
                t!(
                    "stats.of_limit",
                    used = human_bytes(last.mem_used),
                    limit = human_bytes(last.mem_limit)
                )
                .into_owned()
            } else {
                human_bytes(last.mem_used)
            },
        )],
        Some(sparkline(
            &d.stats.mem_series(),
            (last.mem_limit > 0).then_some(last.mem_limit as f64),
        )),
    );
    let network = card(
        t!("stats.network").into_owned(),
        vec![
            (Some(t!("stats.rx").into_owned()), human_bytes(last.net_rx)),
            (Some(t!("stats.tx").into_owned()), human_bytes(last.net_tx)),
        ],
        None,
    );
    let disk = card(
        t!("stats.disk").into_owned(),
        vec![
            (
                Some(t!("stats.read").into_owned()),
                human_bytes(last.blk_read),
            ),
            (
                Some(t!("stats.write").into_owned()),
                human_bytes(last.blk_write),
            ),
        ],
        None,
    );
    // A short dock scrolls instead of cutting the cards.
    scrollable(
        column![
            row![cpu, memory].spacing(SPACE_3),
            row![network, disk].spacing(SPACE_3),
        ]
        .spacing(SPACE_3)
        .padding(SPACE_4),
    )
    .direction(style::thin_scrollbar())
    .style(style::scrollable_thin)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn notice<'a>(message: String) -> Element<'a, Message> {
    center(text(message).style(style::text::muted))
        .padding(SPACE_4)
        .into()
}

/// The Stats tab of the dock. `running` is whether the container is running right now.
pub fn view<'a>(d: &'a DetailState, running: bool) -> Element<'a, Message> {
    match d.stats.latest() {
        // Stopped: the numbers of the last run would only mislead.
        _ if !running => notice(t!("stats.stopped").into_owned()),
        None => notice(t!("stats.waiting").into_owned()),
        Some(last) => cards(d, last),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docker::FakeClient;
    use crate::model::RawStats;

    fn raw() -> RawStats {
        RawStats {
            cpu_total: 0,
            precpu_total: 0,
            system: 0,
            presystem: 0,
            online_cpus: 2,
            mem_usage: 1,
            mem_inactive_file: 0,
            mem_limit: 0,
            net_rx: 0,
            net_tx: 0,
            blk_read: 0,
            blk_write: 0,
        }
    }

    #[tokio::test]
    async fn stats_messages_carry_samples_then_end_with_the_stream() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let client: Arc<dyn DockerClient> = fake.clone();
        let mut s = Box::pin(stats_messages(client, "worker-id".into(), 3));
        // The container is not running, so nothing is made up: only what is pushed.
        fake.push_stats("worker-id", raw());
        fake.close_streams("worker-id");
        assert!(matches!(
            s.next().await,
            Some(Message::Detail(Msg::StatsSample { generation: 3, .. }))
        ));
        assert!(matches!(
            s.next().await,
            Some(Message::Detail(Msg::StatsEnded { generation: 3, .. }))
        ));
        assert!(s.next().await.is_none());
    }

    #[tokio::test]
    async fn stats_of_a_missing_container_just_end() {
        let client: Arc<dyn DockerClient> = Arc::new(FakeClient::with_sample_data());
        let mut s = Box::pin(stats_messages(client, "nope".into(), 0));
        assert!(matches!(
            s.next().await,
            Some(Message::Detail(Msg::StatsEnded { .. }))
        ));
        assert!(s.next().await.is_none());
    }

    #[test]
    fn view_builds_for_every_state() {
        let mut d = DetailState::new("web-id".into());
        for running in [true, false] {
            let _: Element<'_, Message> = view(&d, running);
        }
        let _ = d.update(Msg::StatsSample {
            id: "web-id".into(),
            generation: 0,
            raw: raw(),
        });
        for running in [true, false] {
            let _: Element<'_, Message> = view(&d, running);
        }
    }
}

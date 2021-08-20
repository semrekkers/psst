use std::{fs::File, io, thread};

use crossbeam_channel::{unbounded, Receiver, Sender};

use crate::{
    audio_file::{AudioFile, RawFileSource},
    audio_player::{load_audio_key, load_audio_path, PlaybackConfig},
    cache::CacheHandle,
    cdn::CdnHandle,
    error::Error,
    item_id::ItemId,
    session::SessionService,
};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct CaptureItem {
    pub item_id: ItemId,
}

impl CaptureItem {
    fn load(
        &self,
        session: &SessionService,
        cdn: CdnHandle,
        cache: CacheHandle,
        config: &PlaybackConfig,
    ) -> Result<LoadedCaptureItem, Error> {
        let path = load_audio_path(self.item_id, session, &cache, config)?;
        let key = load_audio_key(&path, session, &cache)?;
        let file = AudioFile::open(path, cdn, cache)?;
        let source = file.raw_source(key)?;
        Ok(LoadedCaptureItem { source })
    }
}

pub struct LoadedCaptureItem {
    source: RawFileSource,
}

pub struct Capturer {
    state: CapturerState,
    session: SessionService,
    cdn: CdnHandle,
    cache: CacheHandle,
    config: PlaybackConfig,
    event_sender: Sender<CapturerEvent>,
    event_receiver: Receiver<CapturerEvent>,
}

impl Capturer {
    pub fn new(
        session: SessionService,
        cdn: CdnHandle,
        cache: CacheHandle,
        config: PlaybackConfig,
    ) -> Self {
        let (event_sender, event_receiver) = unbounded();
        Self {
            session,
            cdn,
            cache,
            config,
            event_sender,
            event_receiver,
            state: CapturerState::Idle,
        }
    }

    pub fn event_sender(&self) -> Sender<CapturerEvent> {
        self.event_sender.clone()
    }

    pub fn event_receiver(&self) -> Receiver<CapturerEvent> {
        self.event_receiver.clone()
    }

    pub fn handle(&mut self, event: CapturerEvent) {
        match event {
            CapturerEvent::Command(cmd) => {
                self.handle_command(cmd);
            }
            CapturerEvent::Downloaded { item } => {
                self.handle_downloaded(item);
            }
            CapturerEvent::Downloading { .. } => {}
        }
    }

    fn handle_command(&mut self, cmd: CapturerCommand) {
        match cmd {
            CapturerCommand::Download { item } => self.download(item),
            CapturerCommand::Configure { config } => self.configure(config),
        }
    }

    fn download(&mut self, item: CaptureItem) {
        self.event_sender
            .send(CapturerEvent::Downloading { item })
            .expect("Failed to send CapturerEvent::Downloading");
        self.state = CapturerState::Downloading { item };

        let downloading_handle = thread::spawn({
            let event_sender = self.event_sender.clone();
            let session = self.session.clone();
            let cdn = self.cdn.clone();
            let cache = self.cache.clone();
            let config = self.config.clone();
            move || {
                let load_result = item.load(&session, cdn, cache, &config);
                match load_result {
                    Ok(mut loaded_item) => {
                        let mut file = File::create(format!(
                            "/home/sem/Downloads/{}.ogg",
                            item.item_id.to_base62()
                        ))
                        .unwrap();
                        io::copy(&mut loaded_item.source, &mut file).unwrap();
                    }
                    Err(err) => {
                        log::error!("skipping, error while loading: {}", err);
                    }
                };
                event_sender
                    .send(CapturerEvent::Downloaded { item })
                    .expect("Failed to send CapturerEvent::Downloaded");
            }
        });
    }

    fn handle_downloaded(&mut self, item: CaptureItem) {}

    fn configure(&mut self, config: PlaybackConfig) {
        self.config = config;
    }
}

pub enum CapturerCommand {
    Download { item: CaptureItem },
    Configure { config: PlaybackConfig },
}

pub enum CapturerEvent {
    Command(CapturerCommand),
    Downloading { item: CaptureItem },
    Downloaded { item: CaptureItem },
}

enum CapturerState {
    Idle,
    Downloading { item: CaptureItem },
    Invalid,
}

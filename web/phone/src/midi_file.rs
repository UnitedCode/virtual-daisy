//! Bounded, streaming SMF parsing with tempo maps and SMPTE timing.
use midly::{Format, Fps, MetaMessage, MidiMessage, Timing, TrackEventKind};
pub struct Event {
    pub ns: u64,
    pub bytes: Vec<u8>,
}
pub struct Song {
    pub events: Vec<Event>,
    pub duration: u64,
}
enum Kind {
    Midi(Vec<u8>),
    Tempo(u32),
}
pub fn parse(bytes: &[u8]) -> Result<Song, String> {
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("MIDI file exceeds 32 MB".into());
    }
    let (header, tracks) = midly::parse(bytes).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    let mut base = 0u64;
    let mut duration = 0;
    let mut visited = 0;
    for track in tracks {
        let mut tick = base;
        if header.format == Format::Sequential {
            raw.push((tick, raw.len(), Kind::Tempo(500_000)));
        }
        for event in track.map_err(|e| e.to_string())? {
            visited += 1;
            if visited > 1_000_000 {
                return Err("MIDI file exceeds one million events".into());
            }
            let event = event.map_err(|e| e.to_string())?;
            tick = tick
                .checked_add(u64::from(event.delta.as_int()))
                .ok_or("MIDI tick overflow")?;
            let kind = match event.kind {
                TrackEventKind::Midi { channel, message } => {
                    let channel = channel.as_int();
                    let data = match message {
                        MidiMessage::NoteOff { key, vel } => {
                            vec![0x80 | channel, key.as_int(), vel.as_int()]
                        }
                        MidiMessage::NoteOn { key, vel } => {
                            vec![0x90 | channel, key.as_int(), vel.as_int()]
                        }
                        MidiMessage::Aftertouch { key, vel } => {
                            vec![0xa0 | channel, key.as_int(), vel.as_int()]
                        }
                        MidiMessage::Controller { controller, value } => {
                            vec![0xb0 | channel, controller.as_int(), value.as_int()]
                        }
                        MidiMessage::ProgramChange { program } => {
                            vec![0xc0 | channel, program.as_int()]
                        }
                        MidiMessage::ChannelAftertouch { vel } => {
                            vec![0xd0 | channel, vel.as_int()]
                        }
                        MidiMessage::PitchBend { bend } => {
                            let value = bend.0.as_int();
                            vec![0xe0 | channel, (value & 127) as u8, (value >> 7) as u8]
                        }
                    };
                    Some(Kind::Midi(data))
                }
                TrackEventKind::Meta(MetaMessage::Tempo(tempo)) => {
                    if tempo.as_int() == 0 {
                        return Err("MIDI tempo must be nonzero".into());
                    }
                    Some(Kind::Tempo(tempo.as_int()))
                }
                TrackEventKind::SysEx(data) => {
                    if data.len() > 32767 {
                        return Err("MIDI SysEx message exceeds the serial queue".into());
                    }
                    let mut bytes = vec![0xf0];
                    bytes.extend(data);
                    Some(Kind::Midi(bytes))
                }
                TrackEventKind::Escape(data) => {
                    if data.len() > 32768 {
                        return Err("MIDI escape message exceeds the serial queue".into());
                    }
                    Some(Kind::Midi(data.to_vec()))
                }
                _ => None,
            };
            if let Some(kind) = kind {
                raw.push((tick, raw.len(), kind));
            }
            duration = duration.max(tick);
        }
        if header.format == Format::Sequential {
            base = tick;
        }
    }
    raw.sort_by_key(|(tick, order, _)| (*tick, *order));
    let mut tempo = 500_000u32;
    let mut previous = 0;
    let mut numerator = 0u128;
    let mut events = Vec::new();
    let denominator = match header.timing {
        Timing::Metrical(ticks) => u128::from(ticks.as_int()),
        Timing::Timecode(fps, ticks) => {
            u128::from(match fps {
                Fps::Fps24 => 24u32,
                Fps::Fps25 => 25,
                Fps::Fps29 => 30000,
                Fps::Fps30 => 30,
            }) * u128::from(ticks)
        }
    };
    if denominator == 0 {
        return Err("MIDI time division must be nonzero".into());
    }
    let units = |tempo: u32| -> u128 {
        match header.timing {
            Timing::Metrical(_) => u128::from(tempo) * 1000,
            Timing::Timecode(Fps::Fps29, _) => 1_001_000_000_000,
            _ => 1_000_000_000,
        }
    };
    for (tick, _, kind) in raw {
        numerator += u128::from(tick - previous) * units(tempo);
        previous = tick;
        let ns = u64::try_from(numerator / denominator).map_err(|_| "MIDI duration overflow")?;
        match kind {
            Kind::Tempo(value) => tempo = value,
            Kind::Midi(bytes) => events.push(Event { ns, bytes }),
        }
    }
    numerator += u128::from(duration - previous) * units(tempo);
    let duration = u64::try_from(numerator / denominator).map_err(|_| "MIDI duration overflow")?;
    if events.is_empty() {
        return Err("This MIDI file has no playable messages".into());
    }
    Ok(Song { events, duration })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn smf(division: u16, track: &[u8]) -> Vec<u8> {
        let mut bytes = b"MThd\0\0\0\x06\0\0\0\x01".to_vec();
        bytes.extend(division.to_be_bytes());
        bytes.extend(b"MTrk");
        bytes.extend((track.len() as u32).to_be_bytes());
        bytes.extend(track);
        bytes
    }
    #[test]
    fn tempo_changes_and_running_status_preserve_timing() {
        let bytes = smf(
            480,
            &[
                0, 0x91, 60, 100, 0x83, 0x60, 0xff, 0x51, 3, 0x0f, 0x42, 0x40, 0, 0x91, 64, 90,
                0x83, 0x60, 60, 0, 0, 0xff, 0x2f, 0,
            ],
        );
        let song = parse(&bytes).unwrap();
        assert_eq!(
            song.events.iter().map(|e| e.ns).collect::<Vec<_>>(),
            [0, 500_000_000, 1_500_000_000]
        );
        assert_eq!(song.events[2].bytes, [0x91, 60, 0]);
        assert_eq!(song.duration, 1_500_000_000);
    }
    #[test]
    fn smpte_drop_frame_uses_30000_over_1001() {
        let song = parse(&smf(
            0xe350,
            &[0, 0x91, 60, 100, 0x92, 0x60, 0x81, 60, 0, 0, 0xff, 0x2f, 0],
        ))
        .unwrap();
        assert_eq!(song.events[1].ns, 1_001_000_000);
    }
    #[test]
    fn invalid_or_empty_input_is_an_error() {
        assert!(parse(b"invalid").is_err());
        assert!(parse(&smf(0, &[0, 0x91, 60, 100])).is_err());
        assert!(parse(&smf(480, &[0, 0xff, 0x2f, 0])).is_err());
    }
}

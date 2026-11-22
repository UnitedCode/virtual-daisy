//! Phone controls and browser I/O. Instrument behaviour belongs to the ARM image.
use js_sys::{Array, Float32Array, Function, Object, Reflect, Uint32Array, Uint8Array};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::*;
mod controls;
mod midi_file;
mod photo_geometry;
mod resample;

#[wasm_bindgen(raw_module = "./engine.js")]
extern "C" {
    #[wasm_bindgen(js_name=default)]
    fn initialize_engine() -> js_sys::Promise;
    #[wasm_bindgen(js_name=Firmware)]
    type Cpu;
    #[wasm_bindgen(constructor,catch,js_class=Firmware)]
    fn new(image: &Uint8Array) -> Result<Cpu, JsValue>;
    #[wasm_bindgen(method, catch)]
    fn run(this: &Cpu, steps: u32) -> Result<Uint32Array, JsValue>;
    #[wasm_bindgen(method, catch)]
    fn run_for(this: &Cpu, steps: u32, nanoseconds: u64) -> Result<Uint32Array, JsValue>;
    #[wasm_bindgen(method)]
    fn input(this: &Cpu, samples: &Float32Array);
    #[wasm_bindgen(method, catch)]
    fn input_stereo(this: &Cpu, samples: &Float32Array) -> Result<(), JsValue>;
    #[wasm_bindgen(method)]
    fn sample_rate(this: &Cpu) -> f64;
    #[wasm_bindgen(method)]
    fn flash_image(this: &Cpu) -> Uint8Array;
    #[wasm_bindgen(method)]
    fn flash_revision(this: &Cpu) -> u64;
    #[wasm_bindgen(method, catch)]
    fn restore_flash(this: &Cpu, bytes: &Uint8Array) -> Result<(), JsValue>;
    #[wasm_bindgen(method)]
    fn pending_input(this: &Cpu) -> u32;
    #[wasm_bindgen(method)]
    fn virtual_time_ns(this: &Cpu) -> u64;
    #[wasm_bindgen(method)]
    fn take_audio(this: &Cpu) -> Float32Array;
    #[wasm_bindgen(method)]
    fn oled(this: &Cpu) -> Uint8Array;
    #[wasm_bindgen(method)]
    fn stats(this: &Cpu) -> String;
    #[wasm_bindgen(method, catch)]
    fn key(this: &Cpu, index: u8, pressed: bool) -> Result<(), JsValue>;
    #[wasm_bindgen(method)]
    fn encoder(this: &Cpu, steps: i32);
    #[wasm_bindgen(method, catch)]
    fn encoder_button(this: &Cpu, pressed: bool) -> Result<(), JsValue>;
    #[wasm_bindgen(method, catch)]
    fn handset(this: &Cpu, lifted: bool) -> Result<(), JsValue>;
    #[wasm_bindgen(method, catch)]
    fn mic_gain(this: &Cpu, value: f32) -> Result<(), JsValue>;
    #[wasm_bindgen(method, catch)]
    fn midi(this: &Cpu, bytes: &Uint8Array) -> Result<(), JsValue>;
}
fn get(o: &JsValue, k: &str) -> JsValue {
    Reflect::get(o, &k.into()).unwrap_or(JsValue::UNDEFINED)
}
fn set(o: &JsValue, k: &str, v: impl Into<JsValue>) {
    Reflect::set(o, &k.into(), &v.into()).unwrap();
}
fn object(kind: &str) -> Object {
    let o = Object::new();
    set(&o, "kind", kind);
    o
}
fn call(o: &JsValue, k: &str, args: &Array) -> Result<JsValue, JsValue> {
    get(o, k).dyn_into::<Function>()?.apply(o, args)
}
fn doc() -> Document {
    window().unwrap().document().unwrap()
}
fn el(id: &str) -> Element {
    doc().get_element_by_id(id).unwrap()
}
fn text(id: &str, s: &str) {
    el(id).set_text_content(Some(s));
}
fn report(e: JsValue) {
    web_sys::console::error_1(&e);
    text("status", "Stopped. Power off, then on.");
    text(
        "error-detail",
        &e.as_string().unwrap_or_else(|| format!("{e:?}")),
    );
}
fn bind(target: &EventTarget, name: &str, f: impl FnMut(Event) + 'static) {
    let c = Closure::<dyn FnMut(Event)>::new(f);
    target
        .add_event_listener_with_callback(name, c.as_ref().unchecked_ref())
        .unwrap();
    c.forget();
}
#[derive(Clone, Copy)]
struct DialDrag {
    origin: i32,
    last: i32,
    moved: bool,
}
fn gain_changed() {
    let input: HtmlInputElement = el("gain").unchecked_into();
    let value = input.value_as_number();
    text("gain-value", &format!("{:.0}%", value * 100.));
    let _ = input.set_attribute("aria-valuetext", &format!("{:.0} percent", value * 100.));
    command("gain", "value", value);
}
fn bind_dial(id: &'static str, encoder: bool) {
    let state: Rc<RefCell<Option<DialDrag>>> = Rc::new(RefCell::new(None));
    let down = state.clone();
    bind(el(id).as_ref(), "pointerdown", move |e| {
        e.prevent_default();
        let p: PointerEvent = e.unchecked_into();
        let target: Element = p.current_target().unwrap().unchecked_into();
        let _ = target.set_pointer_capture(p.pointer_id());
        *down.borrow_mut() = Some(DialDrag {
            origin: p.client_y(),
            last: p.client_y(),
            moved: false,
        });
    });
    let movement = state.clone();
    bind(el(id).as_ref(), "pointermove", move |e| {
        let p: PointerEvent = e.unchecked_into();
        if let Some(drag) = movement.borrow_mut().as_mut() {
            let delta = drag.last - p.client_y();
            if (p.client_y() - drag.origin).abs() > 4 {
                drag.moved = true;
            }
            if drag.moved && delta.abs() >= 4 {
                let steps = delta / 4;
                drag.last -= steps * 4;
                let _ = el(if encoder { "encoder" } else { "mic-dial" })
                    .class_list()
                    .add_1("dragging");
                if encoder {
                    command("encoder", "steps", steps);
                } else {
                    let input: HtmlInputElement = el("gain").unchecked_into();
                    input.set_value_as_number(
                        (input.value_as_number() + f64::from(steps) * 0.04).clamp(0., 2.),
                    );
                    gain_changed();
                }
            }
        }
    });
    for event in ["pointerup", "pointercancel", "lostpointercapture"] {
        let up = state.clone();
        bind(el(id).as_ref(), event, move |_| {
            if let Some(drag) = up.borrow_mut().take() {
                let _ = el(if encoder { "encoder" } else { "mic-dial" })
                    .class_list()
                    .remove_1("dragging");
                if encoder && !drag.moved && event == "pointerup" {
                    command("encoder_button", "value", true);
                    command("encoder_button", "value", false);
                }
            }
        });
    }
}
struct App {
    worker: Worker,
    context: AudioContext,
    node: AudioWorkletNode,
    stream: Option<MediaStream>,
    lifted: bool,
    tone: Option<(OscillatorNode, GainNode)>,
    midi: JsValue,
    outputs: Vec<JsValue>,
    output_index: Option<usize>,
    input_handlers: Vec<(JsValue, Closure<dyn FnMut(JsValue)>)>,
}
thread_local! {static APP:RefCell<Option<App>>=const{RefCell::new(None)};}
fn send(message: &JsValue) {
    APP.with(|s| {
        if let Some(a) = s.borrow().as_ref() {
            if let Err(e) = a.worker.post_message(message) {
                report(e);
            }
        }
    });
}
fn command(kind: &str, key: &str, value: impl Into<JsValue>) {
    let value = value.into();
    let m = object(kind);
    set(&m, key, value);
    send(&m);
}
fn button(index: u8, pressed: bool) {
    if APP.with(|s| s.borrow().is_none()) {
        return;
    }
    let m = object("key");
    set(&m, "index", index);
    set(&m, "pressed", pressed);
    send(&m);
    if let Ok(Some(e)) = doc().query_selector(&format!("[data-key='{index}']")) {
        let _ = e.class_list().toggle_with_force("pressed", pressed);
    }
}
fn lift() {
    APP.with(|s| {
        if let Some(a) = s.borrow_mut().as_mut() {
            a.lifted = !a.lifted;
            let m = object("handset");
            set(&m, "value", a.lifted);
            let _ = a.worker.post_message(&m);
            let _ = el("handset")
                .class_list()
                .toggle_with_force("lifted", a.lifted);
            let _ = el("phone")
                .class_list()
                .toggle_with_force("offhook", a.lifted);
            let _ = el("handset")
                .set_attribute("aria-pressed", if a.lifted { "true" } else { "false" });
            let _ = el("handset").set_attribute(
                "aria-label",
                if a.lifted {
                    "Replace handset"
                } else {
                    "Lift handset"
                },
            );
            text(
                "hook-status",
                if a.lifted {
                    "RECEIVER LIFTED"
                } else {
                    "RECEIVER ON HOOK"
                },
            );
        }
    });
}
fn screen(bytes: &Uint8Array) {
    let canvas: HtmlCanvasElement = el("oled").unchecked_into();
    let c: CanvasRenderingContext2d = canvas.get_context("2d").unwrap().unwrap().unchecked_into();
    c.set_fill_style_str("#081417");
    c.fill_rect(0., 0., 512., 128.);
    c.set_fill_style_str("#a7d6ef");
    for (i, b) in bytes.to_vec().iter().enumerate() {
        for bit in 0..8 {
            if b & (1 << bit) != 0 {
                c.fill_rect(
                    (i % 128 * 4) as f64,
                    ((i / 128 * 8 + bit) * 4) as f64,
                    4.,
                    4.,
                );
            }
        }
    }
}
fn enable() {
    let _ = el("power").remove_attribute("disabled");
    for id in [
        "handset",
        "encoder",
        "enc-down",
        "enc-up",
        "gain",
        "microphone",
        "midi",
        "tone",
        "midi-file",
    ] {
        let _ = el(id).remove_attribute("disabled");
    }
    for n in 0..12 {
        let _ = doc()
            .query_selector(&format!("[data-key='{n}']"))
            .unwrap()
            .unwrap()
            .remove_attribute("disabled");
    }
    let _ = el("power-led").class_list().add_1("on");
    let _ = el("phone").class_list().add_1("powered");
    let _ = el("power").set_attribute("data-on", "true");
    let _ = el("power").set_attribute("aria-label", "Power off");
}
fn configuration() {
    let m = object("configure");
    let gain: HtmlInputElement = el("gain").unchecked_into();
    set(&m, "gain", gain.value_as_number());
    APP.with(|s| {
        if let Some(a) = s.borrow().as_ref() {
            set(&m, "rate", a.context.sample_rate());
        }
    });
    if let Ok(Some(storage)) = window().unwrap().local_storage() {
        if let Ok(Some(saved)) = storage.get_item("synthphone-nor-v1") {
            if saved.len() % 2 == 0 && saved.len() <= 20_000_000 {
                let data: Option<Vec<u8>> = saved
                    .as_bytes()
                    .chunks_exact(2)
                    .map(|pair| {
                        std::str::from_utf8(pair)
                            .ok()
                            .and_then(|s| u8::from_str_radix(s, 16).ok())
                    })
                    .collect();
                if let Some(data) = data {
                    set(&m, "flash", Uint8Array::from(data.as_slice()));
                }
            }
        }
    }
    send(&m);
}
fn save_flash(bytes: &Uint8Array) {
    let result = (|| -> Result<(), JsValue> {
        let storage = window()
            .unwrap()
            .local_storage()?
            .ok_or_else(|| JsValue::from_str("Storage is unavailable"))?;
        let mut value = String::with_capacity(bytes.length() as usize * 2);
        use std::fmt::Write;
        for byte in bytes.to_vec() {
            write!(value, "{byte:02x}").unwrap();
        }
        storage.set_item("synthphone-nor-v1", &value)
    })();
    text(
        "persistence",
        if result.is_ok() {
            "Settings saved on this device."
        } else {
            "This browser cannot save the phone’s settings. Playing still works."
        },
    );
}
fn power_off() {
    APP.with(|s| {
        if let Some(a) = s.borrow_mut().take() {
            if let Some(port) = a.output_index.and_then(|i| a.outputs.get(i)) {
                silence_output(port);
            }
            a.worker.terminate();
            if !a.midi.is_undefined() {
                set(&a.midi, "onstatechange", JsValue::NULL);
            }
            for (input, _) in &a.input_handlers {
                set(input, "onmidimessage", JsValue::NULL);
            }
            if let Some(stream) = a.stream {
                for track in stream.get_tracks().iter() {
                    track.unchecked_into::<MediaStreamTrack>().stop();
                }
            }
            let _ = a.context.close();
        }
    });
    for id in [
        "handset",
        "encoder",
        "enc-down",
        "enc-up",
        "gain",
        "microphone",
        "midi",
        "tone",
        "midi-file",
        "file-play",
        "file-stop",
    ] {
        let _ = el(id).set_attribute("disabled", "");
    }
    for n in 0..12 {
        let key = doc()
            .query_selector(&format!("[data-key='{n}']"))
            .unwrap()
            .unwrap();
        let _ = key.set_attribute("disabled", "");
        let _ = key.class_list().remove_1("pressed");
    }
    let _ = el("power-led").class_list().remove_1("on");
    let _ = el("phone").class_list().remove_2("powered", "offhook");
    let _ = el("power").set_attribute("data-on", "false");
    let _ = el("power").set_attribute("aria-label", "Power on");
    let _ = el("handset").class_list().remove_1("lifted");
    let _ = el("handset").set_attribute("aria-pressed", "false");
    let _ = el("handset").set_attribute("aria-label", "Lift handset");
    text("hook-status", "RECEIVER ON HOOK");
    let tone: HtmlInputElement = el("tone").unchecked_into();
    tone.set_checked(false);
    text("microphone", "MIC");
    text("status", "Off.");
    text("performance", "Phone off.");
    text(
        "file-status",
        "Load a MIDI file after switching on the phone.",
    );
    text("midi-status", "");
    el("midi-in").set_inner_html("<option value=''>None</option>");
    el("midi-out").set_inner_html("<option value=''>None</option>");
    screen(&Uint8Array::new_with_length(512));
    let _ = el("power").remove_attribute("disabled");
}
async fn power() -> Result<(), JsValue> {
    text("error-detail", "");
    let options = AudioContextOptions::new();
    options.set_latency_hint(&"interactive".into());
    let context = AudioContext::new_with_context_options(&options)?;
    JsFuture::from(context.resume()?).await?;
    JsFuture::from(context.audio_worklet()?.add_module("audio-worklet.js")?).await?;
    let node = AudioWorkletNode::new(&context, "firmware-audio")?;
    node.connect_with_audio_node(&context.destination())?;
    let options = WorkerOptions::new();
    options.set_type(WorkerType::Module);
    let worker = Worker::new_with_options("worker.js", &options)?;
    bind(worker.as_ref(), "message", move |e| {
        let e: MessageEvent = e.unchecked_into();
        let d = e.data();
        match get(&d, "kind").as_string().as_deref() {
            Some("ready") => {
                let connected = APP.with(|s| -> Result<(), JsValue> {
                    let s = s.borrow();
                    let a = s.as_ref().ok_or("Phone was powered off")?;
                    let port = a.node.port()?;
                    let message = object("audio_port");
                    set(&message, "port", JsValue::from(port.clone()));
                    a.worker
                        .post_message_with_transfer(&message, &Array::of1(port.as_ref()))
                });
                if let Err(error) = connected {
                    power_off();
                    report(error);
                    return;
                }
                enable();
                configuration();
                text("status", "Ready.");
            }
            Some("screen") => screen(&get(&d, "bytes").unchecked_into()),
            Some("performance") => {
                let state = APP.with(|s| {
                    s.borrow()
                        .as_ref()
                        .map(|a| {
                            get(a.context.as_ref(), "state")
                                .as_string()
                                .unwrap_or_default()
                        })
                        .unwrap_or_default()
                });
                text(
                    "performance",
                    &format!(
                        "Audio: {state}. {}",
                        get(&d, "text").as_string().unwrap_or_default()
                    ),
                );
                if get(&d, "slow").as_bool() == Some(true) {
                    text("status", "Audio is falling behind. Details in Engine.");
                } else if el("status")
                    .text_content()
                    .is_some_and(|s| s.starts_with("Audio is falling behind"))
                {
                    text("status", "Ready.");
                }
            }
            Some("error") => report(get(&d, "error")),
            Some("flash") => save_flash(&Uint8Array::new(&get(&d, "bytes"))),
            Some("persistence") => text(
                "persistence",
                &get(&d, "text").as_string().unwrap_or_default(),
            ),
            Some("file_status") => {
                text(
                    "file-status",
                    &get(&d, "text").as_string().unwrap_or_default(),
                );
                if get(&d, "loaded").is_truthy() {
                    let _ = el("file-play").remove_attribute("disabled");
                    let _ = el("file-stop").remove_attribute("disabled");
                }
            }
            Some("midi_out") => midi_output(&Uint8Array::new(&get(&d, "bytes"))),
            Some("shutdown") => {
                save_flash(&Uint8Array::new(&get(&d, "bytes")));
                power_off();
            }
            _ => {}
        }
    });
    bind(worker.as_ref(), "error", |_| {
        report("Could not start the audio worker. Reload to try again.".into())
    });
    let sample_rate = context.sample_rate();
    APP.with(|s| {
        *s.borrow_mut() = Some(App {
            worker,
            context,
            node,
            stream: None,
            lifted: false,
            tone: None,
            midi: JsValue::UNDEFINED,
            outputs: vec![],
            output_index: None,
            input_handlers: vec![],
        })
    });
    let _ = sample_rate;
    Ok(())
}
async fn microphone() -> Result<(), JsValue> {
    let Some(context) = APP.with(|s| s.borrow().as_ref().map(|a| a.context.clone())) else {
        return Ok(());
    };
    let existing = APP.with(|s| s.borrow().as_ref().is_some_and(|a| a.stream.is_some()));
    if existing {
        APP.with(|s| {
            let mut s = s.borrow_mut();
            let a = s.as_mut().unwrap();
            if let Some(stream) = a.stream.take() {
                for track in stream.get_tracks().iter() {
                    track.unchecked_into::<MediaStreamTrack>().stop();
                }
            }
        });
        text("microphone", "MIC");
        return Ok(());
    }
    let options = MediaStreamConstraints::new();
    let audio = Object::new();
    for key in ["echoCancellation", "noiseSuppression", "autoGainControl"] {
        set(&audio, key, false);
    }
    options.set_audio(&audio);
    options.set_video(&JsValue::FALSE);
    let stream: MediaStream = JsFuture::from(
        window()
            .unwrap()
            .navigator()
            .media_devices()?
            .get_user_media_with_constraints(&options)?,
    )
    .await?
    .dyn_into()?;
    let attached = APP.with(|s| -> Result<bool, JsValue> {
        let mut s = s.borrow_mut();
        let Some(a) = s
            .as_mut()
            .filter(|a| Object::is(a.context.as_ref(), context.as_ref()) && a.stream.is_none())
        else {
            return Ok(false);
        };
        let source = a.context.create_media_stream_source(&stream)?;
        source.connect_with_audio_node(&a.node)?;
        a.stream = Some(stream.clone());
        Ok(true)
    });
    if !matches!(attached, Ok(true)) {
        for track in stream.get_tracks().iter() {
            track.unchecked_into::<MediaStreamTrack>().stop();
        }
        return attached.map(|_| ());
    }
    text("microphone", "MIC ON");
    Ok(())
}
fn midi_route(data: &JsValue) {
    let bytes = Uint8Array::new(&get(data, "data"));
    let m = object("midi");
    set(&m, "bytes", &bytes);
    send(&m);
    let thru: HtmlInputElement = el("thru").unchecked_into();
    if thru.checked() {
        let out: HtmlSelectElement = el("midi-out").unchecked_into();
        if let Ok(index) = out.value().parse::<usize>() {
            APP.with(|s| {
                if let Some(o) = s.borrow().as_ref().and_then(|a| a.outputs.get(index)) {
                    let args = Array::new();
                    args.push(&bytes);
                    let _ = call(o, "send", &args);
                }
            });
        }
    }
}
fn midi_output(bytes: &Uint8Array) {
    APP.with(|s| {
        if let Some(o) = s
            .borrow()
            .as_ref()
            .and_then(|a| a.output_index.and_then(|index| a.outputs.get(index)))
        {
            let args = Array::new();
            args.push(bytes);
            let _ = call(o, "send", &args);
        }
    });
}
fn silence_output(port: &JsValue) {
    for channel in 0..16 {
        let args = Array::new();
        args.push(&Uint8Array::from([0xb0 | channel, 123, 0].as_slice()));
        let _ = call(port, "send", &args);
    }
}
async fn load_file(file: File) -> Result<(), JsValue> {
    if file.size() > 32. * 1024. * 1024. {
        text("file-status", "Choose a MIDI file smaller than 32 MB.");
        return Ok(());
    }
    text("file-status", &format!("Reading {}…", file.name()));
    let bytes = Uint8Array::new(&JsFuture::from(file.array_buffer()).await?);
    let m = object("file");
    set(&m, "bytes", bytes);
    set(&m, "name", file.name());
    send(&m);
    Ok(())
}
fn refresh_midi() -> Result<(), JsValue> {
    APP.with(|s| -> Result<(), JsValue> {
        let mut s = s.borrow_mut();
        let Some(a) = s.as_mut() else {
            return Ok(());
        };
        if let Some(port) = a.output_index.and_then(|index| a.outputs.get(index)) {
            silence_output(port);
        }
        a.output_index = None;
        for (input, _) in &a.input_handlers {
            set(input, "onmidimessage", JsValue::NULL);
        }
        a.input_handlers.clear();
        a.outputs.clear();
        el("midi-in").set_inner_html("<option value=''>None</option>");
        el("midi-out").set_inner_html("<option value=''>None</option>");
        for (key, select) in [("inputs", "midi-in"), ("outputs", "midi-out")] {
            let iter = call(&get(&a.midi, key), "values", &Array::new())?;
            for (n, item) in js_sys::try_iter(&iter)?.unwrap().enumerate() {
                let item = item?;
                let option = doc().create_element("option")?;
                option.set_attribute("value", &n.to_string())?;
                option.set_text_content(Some(
                    &get(&item, "name")
                        .as_string()
                        .unwrap_or_else(|| format!("MIDI device {}", n + 1)),
                ));
                el(select).append_child(&option)?;
                if key == "outputs" {
                    a.outputs.push(item);
                } else {
                    let callback = Closure::<dyn FnMut(JsValue)>::new(move |event| {
                        let select: HtmlSelectElement = el("midi-in").unchecked_into();
                        if select.value() == n.to_string() {
                            midi_route(&event);
                        }
                    });
                    set(&item, "onmidimessage", callback.as_ref().clone());
                    a.input_handlers.push((item, callback));
                }
            }
        }
        text(
            "midi-status",
            "MIDI connected. Choose an input and an optional thru output.",
        );
        Ok(())
    })
}
async fn midi_connect() -> Result<(), JsValue> {
    let Some(context) = APP.with(|s| s.borrow().as_ref().map(|a| a.context.clone())) else {
        return Ok(());
    };
    let navigator: JsValue = window().unwrap().navigator().into();
    let request = get(&navigator, "requestMIDIAccess");
    if !request.is_function() {
        text(
            "midi-status",
            "This browser does not offer Web MIDI. Try a browser that supports it.",
        );
        return Ok(());
    }
    let access = JsFuture::from(
        call(&navigator, "requestMIDIAccess", &Array::new())?.dyn_into::<js_sys::Promise>()?,
    )
    .await?;
    if !APP.with(|s| {
        s.borrow()
            .as_ref()
            .is_some_and(|a| Object::is(a.context.as_ref(), context.as_ref()))
    }) {
        return Ok(());
    }
    let callback = Closure::<dyn FnMut(JsValue)>::new(|_| {
        if let Err(e) = refresh_midi() {
            report(e);
        }
    });
    set(&access, "onstatechange", callback.into_js_value());
    APP.with(|s| s.borrow_mut().as_mut().unwrap().midi = access);
    refresh_midi()
}
#[wasm_bindgen(start)]
pub fn start() {
    if window().is_none() {
        return;
    }
    photo_geometry::align_controls(&doc());
    bind(el("zoom").as_ref(), "click", |_| {
        let zoom = el("zoom").get_attribute("aria-pressed").as_deref() != Some("true");
        let _ = el("workbench")
            .class_list()
            .toggle_with_force("zoomed", zoom);
        let _ = el("zoom").set_attribute("aria-pressed", if zoom { "true" } else { "false" });
        text("zoom", if zoom { "Fit" } else { "Zoom +" });
        if zoom {
            let area: HtmlElement = el("workbench").unchecked_into();
            area.set_scroll_left(
                (f64::from(el("phone").client_width()) * 0.48
                    - f64::from(area.client_width()) * 0.5)
                    .max(0.) as i32,
            );
            area.set_scroll_top(
                (f64::from(el("phone").client_height()) * 0.48
                    - f64::from(area.client_height()) * 0.45)
                    .max(0.) as i32,
            );
        }
    });
    bind(el("midi-out").as_ref(), "change", |_| {
        let select: HtmlSelectElement = el("midi-out").unchecked_into();
        APP.with(|s| {
            if let Some(a) = s.borrow_mut().as_mut() {
                if let Some(port) = a.output_index.and_then(|index| a.outputs.get(index)) {
                    silence_output(port);
                }
                a.output_index = select.value().parse().ok();
            }
        });
    });
    bind(el("midi-in").as_ref(), "change", |_| {
        let bytes: Vec<u8> = (0..16)
            .flat_map(|channel| [0xb0 | channel, 123, 0])
            .collect();
        let message = object("midi");
        set(&message, "bytes", Uint8Array::from(bytes.as_slice()));
        send(&message);
        let thru: HtmlInputElement = el("thru").unchecked_into();
        if thru.checked() {
            APP.with(|s| {
                if let Some(port) = s
                    .borrow()
                    .as_ref()
                    .and_then(|a| a.output_index.and_then(|i| a.outputs.get(i)))
                {
                    silence_output(port);
                }
            });
        }
    });
    bind(el("thru").as_ref(), "change", |_| {
        let thru: HtmlInputElement = el("thru").unchecked_into();
        if !thru.checked() {
            APP.with(|s| {
                if let Some(port) = s
                    .borrow()
                    .as_ref()
                    .and_then(|a| a.output_index.and_then(|i| a.outputs.get(i)))
                {
                    silence_output(port);
                }
            });
        }
    });
    bind(el("midi-file").as_ref(), "change", |_| {
        let input: HtmlInputElement = el("midi-file").unchecked_into();
        if let Some(file) = input.files().and_then(|f| f.get(0)) {
            spawn_local(async {
                if let Err(e) = load_file(file).await {
                    text("file-status", &format!("File could not be read: {e:?}"));
                }
            });
        }
    });
    bind(el("file-play").as_ref(), "click", |_| {
        command("file_play", "value", true)
    });
    bind(el("file-stop").as_ref(), "click", |_| {
        command("file_stop", "value", true)
    });
    bind(doc().as_ref(), "dragover", |e| {
        e.prevent_default();
    });
    bind(doc().as_ref(), "drop", |e| {
        e.prevent_default();
        let e: DragEvent = e.unchecked_into();
        if let Some(file) = e
            .data_transfer()
            .and_then(|d| d.files())
            .and_then(|files| files.get(0))
        {
            if APP.with(|s| s.borrow().is_some()) {
                spawn_local(async {
                    if let Err(e) = load_file(file).await {
                        text("file-status", &format!("File could not be read: {e:?}"));
                    }
                });
            } else {
                text("status", "Switch on the phone before dropping a MIDI file.");
            }
        }
    });
    bind(el("power").as_ref(), "click", |_| {
        if APP.with(|s| s.borrow().is_some()) {
            command("shutdown", "value", true);
            let _ = el("power").set_attribute("disabled", "");
            return;
        }
        let _ = el("power").set_attribute("disabled", "");
        text("status", "Starting…");
        spawn_local(async {
            if let Err(e) = power().await {
                report(e);
                let _ = el("power").remove_attribute("disabled");
            }
        });
    });
    bind(el("handset").as_ref(), "click", |_| lift());
    for i in 0..12 {
        let key = doc()
            .query_selector(&format!("[data-key='{i}']"))
            .unwrap()
            .unwrap();
        bind(key.as_ref(), "pointerdown", move |e| {
            e.prevent_default();
            let p: PointerEvent = e.unchecked_into();
            let target: Element = p.current_target().unwrap().unchecked_into();
            let _ = target.set_pointer_capture(p.pointer_id());
            button(i, true);
        });
        for event in ["pointerup", "pointercancel", "lostpointercapture"] {
            bind(key.as_ref(), event, move |_| button(i, false));
        }
    }
    bind(el("enc-down").as_ref(), "click", |_| {
        command("encoder", "steps", -1)
    });
    bind(el("enc-up").as_ref(), "click", |_| {
        command("encoder", "steps", 1)
    });
    bind_dial("encoder", true);
    bind_dial("gain", false);
    bind(el("encoder").as_ref(), "wheel", |e| {
        let e: WheelEvent = e.unchecked_into();
        e.prevent_default();
        command("encoder", "steps", if e.delta_y() > 0. { 1 } else { -1 });
    });
    bind(el("gain").as_ref(), "input", |_| {
        gain_changed();
    });
    bind(el("microphone").as_ref(), "click", |_| {
        spawn_local(async {
            if let Err(e) = microphone().await {
                text("status", &format!("Microphone unavailable: {e:?}"));
            }
        })
    });
    bind(el("tone").as_ref(), "change", |_| {
        let input: HtmlInputElement = el("tone").unchecked_into();
        let checked = input.checked();
        let result = APP.with(|s| -> Result<(), JsValue> {
            if let Some(a) = s.borrow_mut().as_mut() {
                if checked {
                    let tone = a.context.create_oscillator()?;
                    tone.frequency().set_value(220.);
                    let gain = a.context.create_gain()?;
                    gain.gain().set_value(0.16);
                    tone.connect_with_audio_node(&gain)?;
                    gain.connect_with_audio_node(&a.node)?;
                    tone.start()?;
                    a.tone = Some((tone, gain));
                } else if let Some((tone, gain)) = a.tone.take() {
                    tone.stop()?;
                    gain.disconnect()?;
                }
            }
            Ok(())
        });
        if let Err(e) = result {
            report(e);
        }
    });
    bind(el("midi").as_ref(), "click", |_| {
        spawn_local(async {
            if let Err(e) = midi_connect().await {
                text("midi-status", &format!("MIDI unavailable: {e:?}"));
            }
        })
    });
    for name in ["keydown", "keyup"] {
        let down = name == "keydown";
        bind(doc().as_ref(), name, move |e| {
            let e: KeyboardEvent = e.unchecked_into();
            if e.repeat() || e.ctrl_key() || e.meta_key() || e.alt_key() {
                return;
            }
            if let Some(target) = e.target().and_then(|t| t.dyn_into::<Element>().ok()) {
                if matches!(target.tag_name().as_str(), "INPUT" | "SELECT" | "TEXTAREA") {
                    return;
                }
                if target.tag_name() == "BUTTON" && matches!(e.key().as_str(), " " | "Enter") {
                    if let Some(index) = target
                        .get_attribute("data-key")
                        .and_then(|n| n.parse::<u8>().ok())
                    {
                        e.prevent_default();
                        button(index, down);
                    } else if target.id() == "encoder" {
                        e.prevent_default();
                        command("encoder_button", "value", down);
                    }
                    return;
                }
            }
            let key = e.key();
            if let Some(index) = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "*", "0", "#"]
                .iter()
                .position(|v| *v == key)
            {
                e.prevent_default();
                button(index as u8, down);
            } else {
                match key.as_str() {
                    "ArrowLeft" | "ArrowDown" if down => {
                        e.prevent_default();
                        command("encoder", "steps", -1);
                    }
                    "ArrowRight" | "ArrowUp" if down => {
                        e.prevent_default();
                        command("encoder", "steps", 1);
                    }
                    "Enter" => {
                        e.prevent_default();
                        command("encoder_button", "value", down);
                    }
                    " " if down => {
                        e.prevent_default();
                        lift();
                    }
                    _ => {}
                }
            }
        });
    }
    bind(window().unwrap().as_ref(), "blur", |_| {
        for n in 0..12 {
            button(n, false);
        }
        command("encoder_button", "value", false);
    });
    text("status", "Power on, then pick up.");
    let _ = el("power").remove_attribute("disabled");
}

// The worker owns the only emulated instrument and all its mutable state.
struct Machine {
    cpu: Cpu,
    rate: f64,
    source_rate: f64,
    input: VecDeque<f32>,
    input_resampler: resample::Resampler,
    output_resampler: resample::Resampler,
    samples: u64,
    start: f64,
    last_ui: f64,
    underruns: f64,
    failed: bool,
    switches: [controls::Switch; 13],
    peak: f32,
    configured: bool,
    flash_revision: u64,
    song: Option<midi_file::Song>,
    song_cursor: usize,
    song_start: u64,
    song_playing: bool,
    output: Vec<f32>,
    audio_port: Option<MessagePort>,
    tick_pending: bool,
}
fn queue_tick(machine: &mut Machine, port: &MessagePort) {
    if !machine.tick_pending && !machine.failed {
        machine.tick_pending = true;
        let _ = port.post_message(&JsValue::NULL);
    }
}
fn receive_mic(machine: &mut Machine, data: &JsValue) {
    machine.underruns = get(data, "underruns").as_f64().unwrap_or(0.);
    let samples = Float32Array::new(&get(data, "mic")).to_vec();
    machine.input.extend(machine.input_resampler.process(
        &samples,
        machine.rate,
        machine.source_rate,
    ));
    while machine.input.len() > 8192 {
        machine.input.pop_front();
    }
}
fn start_capture(port: &MessagePort) -> Result<(), JsValue> {
    let start = Object::new();
    set(&start, "start", true);
    port.post_message(&start)
}
fn now() -> f64 {
    js_sys::Date::now()
}
fn post(scope: &DedicatedWorkerGlobalScope, message: &JsValue) {
    let _ = scope.post_message(message);
}
fn fault(scope: &DedicatedWorkerGlobalScope, e: JsValue) {
    let m = object("error");
    set(
        &m,
        "error",
        e.as_string().unwrap_or_else(|| format!("{e:?}")),
    );
    post(scope, &m);
}
fn all_notes_off(
    cpu: &Cpu,
    scope: &DedicatedWorkerGlobalScope,
    controller: u8,
) -> Result<(), JsValue> {
    for channel in 0..16 {
        let bytes = Uint8Array::from([0xb0 | channel, controller, 0].as_slice());
        cpu.midi(&bytes)?;
        let message = object("midi_out");
        set(&message, "bytes", bytes);
        post(scope, &message);
    }
    Ok(())
}
fn tick(scope: DedicatedWorkerGlobalScope, state: Rc<RefCell<Machine>>, port: MessagePort) {
    let result = (|| -> Result<(), JsValue> {
        let mut m = state.borrow_mut();
        m.tick_pending = false;
        if m.failed {
            return Ok(());
        }
        if !m.configured {
            return Ok(());
        }
        if m.song_playing {
            let elapsed = m.cpu.virtual_time_ns().saturating_sub(m.song_start);
            loop {
                let due = m
                    .song
                    .as_ref()
                    .and_then(|song| song.events.get(m.song_cursor))
                    .filter(|event| event.ns <= elapsed)
                    .map(|event| event.bytes.clone());
                let Some(bytes) = due else {
                    break;
                };
                m.cpu.midi(&Uint8Array::from(bytes.as_slice()))?;
                let message = object("midi_out");
                set(&message, "bytes", Uint8Array::from(bytes.as_slice()));
                post(&scope, &message);
                m.song_cursor += 1;
            }
            if m.song
                .as_ref()
                .is_some_and(|s| m.song_cursor == s.events.len() && elapsed >= s.duration)
            {
                m.song_playing = false;
                all_notes_off(&m.cpu, &scope, 123)?;
                let message = object("file_status");
                set(&message, "text", "MIDI file finished. Play to start again.");
                set(&message, "loaded", true);
                post(&scope, &message);
            }
        }
        if !m.input.is_empty() && m.cpu.pending_input() < 4096 {
            let n = m.input.len().min(4096 - m.cpu.pending_input() as usize);
            let samples: Vec<f32> = m.input.drain(..n).flat_map(|sample| [0., sample]).collect();
            m.cpu
                .input_stereo(&Float32Array::from(samples.as_slice()))?;
        }
        // Follow captured codec frames instead of generating future silence.
        // A bounded instruction batch cannot outrun the buffered input.
        if m.cpu.pending_input() < 128 {
            return Ok(());
        }
        for _ in 0..4 {
            let time = m.cpu.virtual_time_ns();
            for index in 0..13 {
                while let Some(pressed) = m.switches[index].due(time) {
                    if index == 12 {
                        m.cpu.encoder_button(pressed)?;
                    } else {
                        m.cpu.key(index as u8, pressed)?;
                    }
                }
            }
            if m.cpu.pending_input() < 128 {
                break;
            }
            m.cpu.run_for(100_000, 2_000_000)?;
        }
        let pcm = m.cpu.take_audio().to_vec();
        m.peak = pcm.iter().fold(0f32, |peak, sample| peak.max(sample.abs()));
        m.samples += pcm.len() as u64;
        if !pcm.is_empty() {
            let physical_rate = m.cpu.sample_rate();
            if physical_rate > 0. {
                m.source_rate = physical_rate;
            }
            let (source_rate, rate) = (m.source_rate, m.rate);
            let output = m.output_resampler.process(&pcm, source_rate, rate);
            m.output.extend(output);
            if m.output.len() >= 256 {
                let output = std::mem::take(&mut m.output);
                let data = object("audio");
                let audio = Float32Array::from(output.as_slice());
                let transfer = Array::of1(audio.buffer().as_ref());
                set(&data, "audio", audio);
                m.audio_port
                    .as_ref()
                    .ok_or("Audio port is not connected")?
                    .post_message_with_transferable(&data, &transfer)?;
            }
        }
        let elapsed = now() - m.start;
        if now() - m.last_ui > 150. {
            m.last_ui = now();
            let data = object("screen");
            set(&data, "bytes", m.cpu.oled());
            post(&scope, &data);
            let revision = m.cpu.flash_revision();
            if revision != m.flash_revision {
                m.flash_revision = revision;
                let data = object("flash");
                set(&data, "bytes", m.cpu.flash_image());
                post(&scope, &data);
            }
            if elapsed > 1200. {
                let speed = m.samples as f64 / m.source_rate / (elapsed / 1000.);
                let data = object("performance");
                set(&data, "slow", speed < 0.95);
                set(
                    &data,
                    "text",
                    format!(
                        "{speed:.2}× real time · {} output samples · peak {:.4} · {} underruns. {}",
                        m.samples,
                        m.peak,
                        m.underruns,
                        m.cpu.stats()
                    ),
                );
                post(&scope, &data);
            }
        }
        Ok(())
    })();
    if let Err(e) = result {
        state.borrow_mut().failed = true;
        fault(&scope, e);
        return;
    }
    let mut m = state.borrow_mut();
    if m.configured && (m.cpu.pending_input() >= 128 || !m.input.is_empty()) {
        queue_tick(&mut m, &port);
    }
}
#[wasm_bindgen]
pub async fn start_worker() -> Result<(), JsValue> {
    JsFuture::from(initialize_engine()).await?;
    let scope: DedicatedWorkerGlobalScope = js_sys::global().unchecked_into();
    let response: Response = JsFuture::from(scope.fetch_with_str("synthphone.bin"))
        .await?
        .dyn_into()?;
    if !response.ok() {
        return Err("Firmware download failed".into());
    }
    let image = Uint8Array::new(&JsFuture::from(response.array_buffer()?).await?);
    let cpu = Cpu::new(&image)?;
    cpu.handset(false)?;
    let state = Rc::new(RefCell::new(Machine {
        cpu,
        rate: 48000.,
        source_rate: 48_187.487,
        input: VecDeque::new(),
        input_resampler: resample::Resampler::default(),
        output_resampler: resample::Resampler::default(),
        samples: 0,
        start: now(),
        last_ui: 0.,
        underruns: 0.,
        failed: false,
        switches: core::array::from_fn(|_| controls::Switch::default()),
        peak: 0.,
        configured: false,
        flash_revision: 0,
        song: None,
        song_cursor: 0,
        song_start: 0,
        song_playing: false,
        output: Vec::new(),
        audio_port: None,
        tick_pending: false,
    }));
    let channel = MessageChannel::new()?;
    let port = channel.port2();
    let s = state.clone();
    let scope_error = scope.clone();
    let wakeup = port.clone();
    bind(scope.as_ref(), "message", move |e| {
        let e: MessageEvent = e.unchecked_into();
        let d = e.data();
        let mut m = s.borrow_mut();
        let value = get(&d, "value");
        let result = match get(&d, "kind").as_string().as_deref() {
            Some("audio_port") => (|| -> Result<(), JsValue> {
                let audio_port: MessagePort = get(&d, "port").dyn_into()?;
                let state = s.clone();
                let wake = wakeup.clone();
                bind(audio_port.as_ref(), "message", move |event| {
                    let event: MessageEvent = event.unchecked_into();
                    let mut machine = state.borrow_mut();
                    if machine.failed {
                        return;
                    }
                    receive_mic(&mut machine, &event.data());
                    queue_tick(&mut machine, &wake);
                });
                audio_port.start();
                if m.configured {
                    start_capture(&audio_port)?;
                }
                m.audio_port = Some(audio_port);
                Ok(())
            })(),
            Some("file") => {
                match midi_file::parse(&Uint8Array::new(&get(&d, "bytes")).to_vec()) {
                    Ok(song) => {
                        if m.song_playing {
                            let _ = all_notes_off(&m.cpu, &scope_error, 120);
                        }
                        let message = object("file_status");
                        set(
                            &message,
                            "text",
                            format!(
                                "{} · {:.1} s · {} MIDI events",
                                get(&d, "name").as_string().unwrap_or_default(),
                                song.duration as f64 / 1e9,
                                song.events.len()
                            ),
                        );
                        set(&message, "loaded", true);
                        post(&scope_error, &message);
                        m.song = Some(song);
                        m.song_cursor = 0;
                        m.song_playing = false;
                    }
                    Err(error) => {
                        let message = object("file_status");
                        set(&message, "text", error);
                        post(&scope_error, &message);
                    }
                }
                Ok(())
            }
            Some("file_play") => {
                if m.song.is_some() {
                    let _ = all_notes_off(&m.cpu, &scope_error, 120);
                    m.song_start = m.cpu.virtual_time_ns();
                    m.song_cursor = 0;
                    m.song_playing = true;
                    let message = object("file_status");
                    set(
                        &message,
                        "text",
                        "Playing through the firmware’s MIDI UART.",
                    );
                    set(&message, "loaded", true);
                    post(&scope_error, &message);
                }
                Ok(())
            }
            Some("file_stop") => {
                m.song_playing = false;
                m.song_cursor = 0;
                let message = object("file_status");
                set(&message, "text", "MIDI file stopped.");
                set(&message, "loaded", m.song.is_some());
                post(&scope_error, &message);
                all_notes_off(&m.cpu, &scope_error, 120)
            }
            Some("configure") => {
                m.rate = get(&d, "rate").as_f64().unwrap_or(48000.);
                let _ = m
                    .cpu
                    .mic_gain(get(&d, "gain").as_f64().unwrap_or(1.) as f32);
                if !m.configured {
                    let flash = get(&d, "flash");
                    if !flash.is_undefined() {
                        if m.cpu.restore_flash(&Uint8Array::new(&flash)).is_err() {
                            let data = object("persistence");
                            set(
                                &data,
                                "text",
                                "Saved settings could not be read. The phone is using defaults.",
                            );
                            post(&scope_error, &data);
                        }
                    }
                    m.configured = true;
                    m.start = now();
                    if let Some(port) = &m.audio_port {
                        start_capture(port)
                    } else {
                        Ok(())
                    }
                } else {
                    Ok(())
                }
            }
            Some("shutdown") => {
                let data = object("shutdown");
                set(&data, "bytes", m.cpu.flash_image());
                post(&scope_error, &data);
                m.failed = true;
                Ok(())
            }
            Some("key") => {
                let index = get(&d, "index").as_f64().unwrap_or(0.) as usize;
                if index < 12 {
                    let pressed = get(&d, "pressed").is_truthy();
                    let now = m.cpu.virtual_time_ns();
                    m.switches[index].set(pressed, now);
                    Ok(())
                } else {
                    Err("Invalid keypad index".into())
                }
            }
            Some("encoder") => {
                m.cpu
                    .encoder(get(&d, "steps").as_f64().unwrap_or(0.) as i32);
                Ok(())
            }
            Some("encoder_button") => {
                let now = m.cpu.virtual_time_ns();
                m.switches[12].set(value.is_truthy(), now);
                Ok(())
            }
            Some("handset") => m.cpu.handset(value.is_truthy()),
            Some("gain") => m.cpu.mic_gain(value.as_f64().unwrap_or(1.) as f32),
            Some("midi") => m.cpu.midi(&Uint8Array::new(&get(&d, "bytes"))),
            _ => Ok(()),
        };
        if let Err(e) = result {
            m.failed = true;
            fault(&scope_error, e);
        } else {
            queue_tick(&mut m, &wakeup);
        }
    });
    let s = state.clone();
    let sc = scope.clone();
    let p = port.clone();
    bind(channel.port1().as_ref(), "message", move |_| {
        tick(sc.clone(), s.clone(), p.clone())
    });
    channel.port1().start();
    let ready = object("ready");
    post(&scope, &ready);
    tick(scope, state, port);
    Ok(())
}

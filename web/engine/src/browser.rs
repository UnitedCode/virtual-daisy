//! Browser harness. Audio is produced exclusively by the ARM firmware.
use crate::Firmware;
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};
fn document() -> web_sys::Document {
    web_sys::window().unwrap().document().unwrap()
}
fn text(id: &str, value: &str) {
    if let Some(e) = document().get_element_by_id(id) {
        e.set_text_content(Some(value));
    }
}
fn error(value: impl core::fmt::Display) {
    text("status", &format!("Stopped: {value}"));
    for id in ["mic-test", "silence-test", "midi-test"] {
        if let Some(button) = document().get_element_by_id(id) {
            let _ = button.remove_attribute("disabled");
        }
    }
}
struct Render {
    firmware: Firmware,
    audio: Vec<f32>,
    start: f64,
    kind: String,
}
fn screen(f: &Firmware) {
    let canvas: HtmlCanvasElement = document()
        .get_element_by_id("oled")
        .unwrap()
        .dyn_into()
        .unwrap();
    let c: CanvasRenderingContext2d = canvas
        .get_context("2d")
        .unwrap()
        .unwrap()
        .dyn_into()
        .unwrap();
    c.set_fill_style_str("#071117");
    c.fill_rect(0., 0., 512., 128.);
    c.set_fill_style_str("#a3f4ff");
    for (i, byte) in f.oled().iter().enumerate() {
        for bit in 0..8 {
            if byte & (1 << bit) != 0 {
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
fn tick(state: Rc<RefCell<Render>>) {
    let finished = {
        let mut s = state.borrow_mut();
        if let Err(e) = s.firmware.run(1_000_000) {
            error(e);
            return;
        }
        let audio = s.firmware.take_audio();
        s.audio.extend(audio);
        screen(&s.firmware);
        text("diagnostics", &s.firmware.stats());
        text(
            "status",
            &format!(
                "Running actual ARM firmware · {} / 12004 samples",
                s.audio.len()
            ),
        );
        s.audio.len() >= 12_004
    };
    if finished {
        let s = state.borrow();
        let elapsed = web_sys::window().unwrap().performance().unwrap().now() - s.start;
        let peak = s.audio.iter().fold(0f32, |p, x| p.max(x.abs()));
        let rms = (s.audio.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>()
            / s.audio.len() as f64)
            .sqrt();
        let rate = s.firmware.sample_rate();
        text("status",&format!("Rendered {} samples · peak {:.5} · RMS {:.5} · {:.2} s processing · {:.2}× real time",s.audio.len(),peak,rms,elapsed/1000.,s.audio.len()as f64/rate/(elapsed/1000.)));
        let wav = wav(&s.audio, rate.round() as u32);
        let bytes = js_sys::Uint8Array::from(wav.as_slice());
        let parts = js_sys::Array::new();
        parts.push(&bytes);
        let options = web_sys::BlobPropertyBag::new();
        options.set_type("audio/wav");
        let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options).unwrap();
        let url = web_sys::Url::create_object_url_with_blob(&blob).unwrap();
        let player = document().get_element_by_id("audio").unwrap();
        if let Some(old) = player.get_attribute("src") {
            let _ = web_sys::Url::revoke_object_url(&old);
        }
        player.set_attribute("src", &url).unwrap();
        let download = document().get_element_by_id("download").unwrap();
        download.set_attribute("href", &url).unwrap();
        download
            .set_attribute("download", &format!("synthphone-{}.wav", s.kind))
            .unwrap();
        download.set_text_content(Some("Download firmware output"));
        for id in ["mic-test", "silence-test", "midi-test"] {
            document()
                .get_element_by_id(id)
                .unwrap()
                .remove_attribute("disabled")
                .unwrap();
        }
    } else {
        let callback = Closure::once_into_js(move || tick(state));
        web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(callback.unchecked_ref(), 0)
            .unwrap();
    }
}
fn wav(samples: &[f32], rate: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    let size = samples.len() as u32 * 4;
    bytes.extend(b"RIFF");
    bytes.extend((size + 36).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(3u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(rate.to_le_bytes());
    bytes.extend((rate * 4).to_le_bytes());
    bytes.extend(4u16.to_le_bytes());
    bytes.extend(32u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(size.to_le_bytes());
    for sample in samples {
        bytes.extend(sample.to_le_bytes());
    }
    bytes
}
#[wasm_bindgen(start)]
pub fn start() {
    if web_sys::window().is_none() {
        return;
    }
    spawn_local(async {
        let result:Result<(),JsValue>=async{
   let response: web_sys::Response=JsFuture::from(web_sys::window().unwrap().fetch_with_str("synthphone.bin")).await?.dyn_into()?;
   if !response.ok(){return Err(JsValue::from_str("Firmware download failed"));}
   let image=Rc::new(js_sys::Uint8Array::new(&JsFuture::from(response.array_buffer()?).await?).to_vec());
   for (id,kind) in [("mic-test","mic"),("silence-test","silence"),("midi-test","midi")] {
    let image=image.clone();let kind=kind.to_string();
    let callback=Closure::<dyn FnMut()>::new(move||{
     let mut firmware=match Firmware::new(&image){Ok(f)=>f,Err(e)=>{error(e);return;}};
     let input:Vec<f32>=(0..24_008).map(|n|if kind=="mic"{(n as f32*220.*std::f32::consts::TAU/48_187.487).sin()*0.2}else{0.}).collect();firmware.input(&input);
     if kind=="midi"{if let Err(e)=firmware.run(2_000_000).and_then(|_|firmware.midi(&[0x91,69,100])){error(e);return;}}
     for id in ["mic-test","silence-test","midi-test"]{document().get_element_by_id(id).unwrap().set_attribute("disabled","").unwrap();}
     let state=Rc::new(RefCell::new(Render{firmware,audio:Vec::new(),start:web_sys::window().unwrap().performance().unwrap().now(),kind:kind.clone()}));tick(state);
    });
    document().get_element_by_id(id).unwrap().add_event_listener_with_callback("click",callback.as_ref().unchecked_ref())?;callback.forget();
    document().get_element_by_id(id).unwrap().remove_attribute("disabled")?;
   }
   text("status","Ready. Each test boots the original firmware and renders ¼ second through its real audio code.");Ok(())
  }.await;
        if let Err(e) = result {
            error(format!("{e:?}"));
        }
    });
}

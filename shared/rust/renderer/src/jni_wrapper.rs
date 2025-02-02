// Copyright 2022 Nathan Prat

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at

//     http://www.apache.org/licenses/LICENSE-2.0

// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// https://github.com/jinleili/wgpu-on-app/blob/master/src/android.rs
// and https://github.com/gfx-rs/wgpu/discussions/1487

use android_logger::Config;
use bevy::prelude::Color;
use common::DisplayStrippedCircuitsPackageBuffers;
use core::ffi::c_void;
use jni::objects::{JClass, JObject, JString, ReleaseMode};
use jni::sys::{jfloatArray, jint, jlong};
use jni::JNIEnv;
use jni_fn::jni_fn;
use log::{debug, info, LevelFilter};
use raw_window_handle::{AndroidNdkWindowHandle, RawWindowHandle};

// #[cfg(target_os = "android")]
use android_logger::FilterBuilder;

use crate::{init_app, vertices_utils::Rect, App};

extern "C" {
    pub fn ANativeWindow_fromSurface(env: JNIEnv, surface: JObject) -> usize;
    // TODO maybe use:ANativeWindow_getFormat?
    pub fn ANativeWindow_getHeight(window_ptr: usize) -> u32;
    pub fn ANativeWindow_getWidth(window_ptr: usize) -> u32;
}

pub fn get_raw_window_handle(env: JNIEnv, surface: JObject) -> (RawWindowHandle, u32, u32) {
    let a_native_window = unsafe { ANativeWindow_fromSurface(env, surface) };
    let mut handle = AndroidNdkWindowHandle::empty();
    handle.a_native_window = a_native_window as *mut c_void;

    let width = unsafe { ANativeWindow_getWidth(a_native_window) };
    let height = unsafe { ANativeWindow_getHeight(a_native_window) };

    (RawWindowHandle::AndroidNdk(handle), width, height)
}

// TODO static state? or return Box<State> in initSurface and store as "long" in Kotlin?
// static mut state: Option<State> = None;size
#[allow(clippy::too_many_arguments)]
fn init_surface(
    env: JNIEnv,
    surface: JObject,
    message_rects: jfloatArray,
    pinpad_rects: jfloatArray,
    pinpad_nb_cols: usize,
    pinpad_nb_rows: usize,
    message_text_color: Color,
    circle_text_color: Color,
    circle_color: Color,
    background_color: Color,
    message_pgarbled_buf: Vec<u8>,
    pinpad_pgarbled_buf: Vec<u8>,
) -> jlong {
    // TODO use loggers.rs(same as substrate-client)
    // WARNING: conflicts with substrate-client/src/loggers.rs
    // only the first one called is taken into account
    android_logger::init_once(
        Config::default()
            .with_max_level(LevelFilter::Info)
            .with_tag("interstellar")
            .with_filter(
                FilterBuilder::new()
                    // useful: wgpu_hal=info
                    .parse("info,jni::crate=debug")
                    .build(),
            ),
    );

    let (handle, width, height) = get_raw_window_handle(env, surface);
    log::debug!(
        "initSurface: got handle! width = {}, height = {}, handle = {:?}",
        width,
        height,
        handle,
    );
    info!("initSurface before new_native");

    let mut message_rects_vec = unsafe {
        convert_rect_float_arr_to_vec_rect(env, message_rects, width as f32, height as f32)
    };
    let pinpad_rects_vec = unsafe {
        convert_rect_float_arr_to_vec_rect(env, pinpad_rects, width as f32, height as f32)
    };
    assert!(
        message_rects_vec.len() == 1,
        "should have only ONE message_rects!",
    );
    assert!(
        pinpad_rects_vec.len() == pinpad_nb_cols * pinpad_nb_rows,
        "pinpad_rects length MUST = pinpad_nb_cols * pinpad_nb_rows!"
    );
    // get the only Rect from "message_rects"; owned
    let message_rect = message_rects_vec.swap_remove(0);
    debug!("init_surface: message_rect: {:?}", message_rect);
    // pinpad: convert the Vec<> into a 2D matrix
    let mut pinpad_rects = ndarray::Array2::<Rect>::default((pinpad_nb_rows, pinpad_nb_cols));
    for row in 0..pinpad_nb_rows {
        for col in 0..pinpad_nb_cols {
            let index = col + row * pinpad_nb_cols;
            debug!(
                "init_surface: col: {:?}, row: {:?}, index: {}",
                col, row, index
            );
            pinpad_rects[[row, col]] = pinpad_rects_vec.get(index).unwrap().clone();
            // swap_remove takes the first(0 in this case), so no need to compute "let index = col + row * pinpad_nb_cols;"
            // pinpad_rects[[row, col]] = pinpad_rects_vec.swap_remove(0);
            // FAIL: the order ends up messed up, which means the "cancel" and "go" button are not in the right place
        }
    }

    // TODO?
    // let size = winit::dpi::PhysicalSize::new(width, height);
    // &awindow,
    //     size,
    //     update_texture_data,
    //     vertices,
    //     indices,
    //     texture_base,
    let mut app = App::new();

    log::debug!("before init_app");
    init_app(
        &mut app,
        message_rect,
        pinpad_rects,
        pinpad_nb_cols,
        pinpad_nb_rows,
        message_text_color,
        circle_text_color,
        circle_color,
        background_color,
        // DEV/DEBUG: offline
        // include_bytes!("../examples/data/message_224x96.pgarbled.stripped.pb.bin").to_vec(),
        // include_bytes!("../examples/data/pinpad_590x50.pgarbled.stripped.pb.bin").to_vec(),
        message_pgarbled_buf,
        pinpad_pgarbled_buf,
        #[cfg(target_os = "android")]
        width,
        #[cfg(target_os = "android")]
        height,
        #[cfg(target_os = "android")]
        handle,
    );

    info!("init_app ok!");

    Box::into_raw(Box::new(app)) as jlong
    // TODO static state?
    // 0
}

/// IMPORTANT: pinpad_rects is assumed to be given from top->bottom, left->right
/// ie pinpad_rects[0] is top left, pinpad_rects[12] is bottom right
///
/// param: surface: SHOULD come from "override fun surfaceCreated(holder: SurfaceHolder)" holder.surface
/// param: circuits_package_ptr: MUST be the returned value from substrate-client/src/jni_wrapper.rs GetCircuits
///     NOTE: the pointer is NOT valid after this function returns!
#[jni_fn("gg.interstellar.wallet.RustWrapper")]
pub unsafe fn initSurface(
    env: JNIEnv,
    _: JClass,
    surface: JObject,
    message_rects: jfloatArray,
    pinpad_rects: jfloatArray,
    pinpad_nb_cols: jint,
    pinpad_nb_rows: jint,
    message_text_color_hex: JString,
    circle_text_color_hex: JString,
    circle_color_hex: JString,
    background_color_hex: JString,
    circuits_package_ptr: jlong,
) -> jlong {
    // USE A Box, that way the pointer is properly cleaned up when exiting this function
    // let circuits_package = &mut *(circuits_package_ptr as *mut DisplayStrippedCircuitsPackageBuffers);
    let display_stripped_circuits_package_buffers: Box<DisplayStrippedCircuitsPackageBuffers> =
        Box::from_raw(circuits_package_ptr as *mut _);

    init_surface(
        env,
        surface,
        message_rects,
        pinpad_rects,
        pinpad_nb_cols.try_into().unwrap(),
        pinpad_nb_rows.try_into().unwrap(),
        Color::hex::<String>(
            env.get_string(message_text_color_hex)
                .expect("Couldn't get java string message_text_color_hex!")
                .into(),
        )
        .unwrap(),
        Color::hex::<String>(
            env.get_string(circle_text_color_hex)
                .expect("Couldn't get java string circle_text_color_hex!")
                .into(),
        )
        .unwrap(),
        Color::hex::<String>(
            env.get_string(circle_color_hex)
                .expect("Couldn't get java string circle_color_hex!")
                .into(),
        )
        .unwrap(),
        Color::hex::<String>(
            env.get_string(background_color_hex)
                .expect("Couldn't get java string background_color_hex!")
                .into(),
        )
        .unwrap(),
        display_stripped_circuits_package_buffers
            .message_pgarbled_buf
            .clone(),
        display_stripped_circuits_package_buffers
            .pinpad_pgarbled_buf
            .clone(),
    )
}

#[jni_fn("gg.interstellar.wallet.RustWrapper")]
pub unsafe fn render(_env: *mut JNIEnv, _: JClass, obj: jlong) {
    // TODO static state?
    let app = &mut *(obj as *mut App);
    // DO NOT use app.run() cf https://github.com/bevyengine/bevy/blob/main/examples/app/custom_loop.rs
    // calling app.run() makes Android display not updating after a few loops.
    // The texture are setup, circuit_evaluate runs a few times and then nothing changes anymore
    // change_texture_message/change_texture_pinpad are NOT called anymore
    // app.run();
    app.update();
}

#[jni_fn("gg.interstellar.wallet.RustWrapper")]
pub unsafe fn cleanup(_env: *mut JNIEnv, _: JClass, obj: jlong) {
    let _obj: Box<App> = Box::from_raw(obj as *mut _);
}

/// Convert a floatArray like [left0, top0, right0, bottom0, left1, top2, right1, bottom1, ...]
/// into vec[Rect(left0, top0, right0, bottom0),Rect(left1, top2, right1, bottom1),...]
///
/// NOTE: will also convert the Coords to match Bevy
/// eg a Rect on the top of screen, full width:
//  0 = {Rect@20731} Rect.fromLTRB(0.0, 0.0, 1080.0, 381.0)
//  message_rects_flattened = {ArrayList@20533}  size = 4
//   0 = {Float@20689} 0.0
//   1 = {Float@20690} 0.0
//   2 = {Float@20691} 1080.0
//   3 = {Float@20692} 381.0
// will be converted to:
// Rect(left:0.0, top: height - 0.0, right: 1080, bottom: height - 381.0)
unsafe fn convert_rect_float_arr_to_vec_rect(
    env: JNIEnv,
    rects_float_array: jfloatArray,
    width: f32,
    height: f32,
) -> Vec<Rect> {
    let rects_floatarr = env
        .get_float_array_elements(rects_float_array, ReleaseMode::NoCopyBack)
        .unwrap();
    assert_ne!(
        rects_floatarr.size().unwrap(),
        0,
        "rects_floatarr is empty!"
    );
    assert_eq!(
        rects_floatarr.size().unwrap() % 4,
        0,
        "rects_floatarr MUST be % 4!"
    );

    let mut rects_vec =
        Vec::<Rect>::with_capacity((rects_floatarr.size().unwrap() / 4).try_into().unwrap());
    for (idx, i) in (0..rects_floatarr.size().unwrap()).step_by(4).enumerate() {
        rects_vec.insert(
            idx,
            Rect::new_to_ndc_android(
                // message_rects_jlist.get(i).unwrap().unwrap().into(),
                // message_rects_jlist.get(i + 1).unwrap().unwrap().into(),
                // message_rects_jlist.get(i + 2).unwrap().unwrap().into(),
                // message_rects_jlist.get(i + 3).unwrap().unwrap().into(),
                *rects_floatarr.as_ptr().offset(i.try_into().unwrap()),
                *rects_floatarr.as_ptr().offset((i + 1).try_into().unwrap()),
                *rects_floatarr.as_ptr().offset((i + 2).try_into().unwrap()),
                *rects_floatarr.as_ptr().offset((i + 3).try_into().unwrap()),
                width,
                height,
            ),
        );
    }

    rects_vec
}

#[cfg(test)]
mod tests {
    use super::*;
    use jni::sys::jfloat;

    // https://github.com/jni-rs/jni-rs/blob/master/tests/util/mod.rs

    #[cfg(target_os = "linux")] // we do not need jni features = ["invocation"] for Android
    fn jvm() -> &'static std::sync::Arc<jni::JavaVM> {
        static mut JVM: Option<std::sync::Arc<jni::JavaVM>> = None;
        static INIT: std::sync::Once = std::sync::Once::new();

        INIT.call_once(|| {
            let jvm_args = jni::InitArgsBuilder::new()
                .version(jni::JNIVersion::V8)
                .option("-Xcheck:jni")
                .build()
                .unwrap_or_else(|e| panic!("{:#?}", e));

            let jvm = jni::JavaVM::new(jvm_args).unwrap_or_else(|e| panic!("{:#?}", e));

            unsafe {
                JVM = Some(std::sync::Arc::new(jvm));
            }
        });

        unsafe { JVM.as_ref().unwrap() }
    }

    #[cfg(test)]
    #[cfg(target_os = "linux")] // we do not need jni features = ["invocation"] for Android
    #[allow(dead_code)]
    pub fn attach_current_thread() -> jni::AttachGuard<'static> {
        jvm()
            .attach_current_thread()
            .expect("failed to attach jvm thread")
    }

    // cf https://github.com/jni-rs/jni-rs/blob/master/tests/jni_api.rs
    #[cfg(target_os = "linux")] // we do not need jni features = ["invocation"] for Android
    #[test]
    pub fn test_convert_rect_float_arr_to_vec_rect() {
        let env = attach_current_thread();

        //     result = {Rect[1]@20529}
        //  0 = {Rect@20731} Rect.fromLTRB(0.0, 0.0, 1080.0, 381.0)
        // message_rects_flattened = {ArrayList@20533}  size = 4
        //  0 = {Float@20689} 0.0
        //  1 = {Float@20690} 0.0
        //  2 = {Float@20691} 1080.0
        //  3 = {Float@20692} 381.0
        let buf: &[jfloat] = &[
            0.0 as jfloat,
            0.0 as jfloat,
            1080.0 as jfloat,
            381.0 as jfloat,
        ];
        let java_array = env
            .new_float_array(4)
            .expect("JNIEnv#new_float_array must create a Java jfloat array with given size");

        // Insert array elements
        let _ = env.set_float_array_region(java_array, 0, buf);

        let res = unsafe { convert_rect_float_arr_to_vec_rect(*env, java_array, 1080., 1920.) };

        assert_eq!(res[0], Rect::new(-0.5625, 1.0, 0.5625, 0.603125))
    }
}

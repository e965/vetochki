//! Android entry point: android-activity's NativeActivity glue calls `android_main`, which hands
//! the activity to the shared event loop in `vetochki`.
#![cfg(target_os = "android")]

use winit::{
    event_loop::EventLoop,
    platform::android::{EventLoopBuilderExtAndroid, activity::AndroidApp},
};

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    vetochki::run(
        EventLoop::builder()
            .with_android_app(app)
            .build()
            .expect("event loop"),
    );
}

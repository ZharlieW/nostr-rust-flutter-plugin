// JNI bridge for Android Service to call Rust directly without Flutter Engine

#[cfg(target_os = "android")]
use jni::JNIEnv;
#[cfg(target_os = "android")]
use jni::objects::{JClass, JString};
#[cfg(target_os = "android")]
use jni::sys::{jboolean, jstring, JNI_FALSE, JNI_TRUE};
use std::sync::OnceLock;
use tokio::runtime::Runtime;

// Global tokio runtime for JNI calls
static RUNTIME: OnceLock<Runtime> = OnceLock::new();

fn get_runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        Runtime::new().expect("Failed to create tokio runtime")
    })
}

/// JNI function to start relay
/// Returns "OK" on success, error message on failure
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_com_aegis_app_NostrRelayJni_startRelay(
    mut env: JNIEnv,
    _class: JClass,
    host: JString,
    port: jni::sys::jint,
    db_path: JString,
) -> jstring {
    // Get string parameters from JNI
    let host_str = match env.get_string(&host) {
        Ok(jstr) => jstr.into(),
        Err(e) => {
            eprintln!("Failed to get host string: {:?}", e);
            let error_msg = env.new_string("ERROR: Failed to get host parameter").unwrap();
            return error_msg.into_raw();
        }
    };
    
    let db_path_str = match env.get_string(&db_path) {
        Ok(jstr) => jstr.into(),
        Err(e) => {
            eprintln!("Failed to get db_path string: {:?}", e);
            let error_msg = env.new_string("ERROR: Failed to get db_path parameter").unwrap();
            return error_msg.into_raw();
        }
    };
    
    let port_u16 = port as u16;
    
    // Use tokio runtime to run async function
    let runtime = get_runtime();
    let relay_result = runtime.block_on(async {
        crate::api::relay::start_relay(host_str, port_u16, db_path_str).await
    });
    
    // Create return string
    match relay_result {
        Ok(_) => {
            match env.new_string("OK") {
                Ok(jstr) => jstr.into_raw(),
                Err(e) => {
                    eprintln!("Failed to create OK string: {:?}", e);
                    std::ptr::null_mut()
                }
            }
        }
        Err(e) => {
            let error_msg = format!("ERROR: {}", e);
            match env.new_string(&error_msg) {
                Ok(jstr) => jstr.into_raw(),
                Err(err) => {
                    eprintln!("Failed to create error string: {:?}", err);
                    std::ptr::null_mut()
                }
            }
        }
    }
}

/// JNI function to check if relay is running
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_com_aegis_app_NostrRelayJni_isRelayRunning(
    _env: JNIEnv,
    _class: JClass,
) -> jboolean {
    if crate::api::relay::is_relay_running() {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

/// JNI function to get relay URL
/// Returns URL string on success, null on failure
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_com_aegis_app_NostrRelayJni_getRelayUrl(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    match crate::api::relay::get_relay_url() {
        Ok(url) => {
            match env.new_string(&url) {
                Ok(jstr) => jstr.into_raw(),
                Err(_) => std::ptr::null_mut(),
            }
        }
        Err(_) => std::ptr::null_mut(),
    }
}


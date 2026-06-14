# Consumer ProGuard rules for the :common library module.
# These are applied when the consuming application module enables minification.
#
# Keep JNI bridge — methods called from native code via RegisterNatives or
# conventional JNI naming must not be stripped.
-keep class com.anyplug.common.bridge.RustBridge {
    native <methods>;
}

# Keep data model classes used across module boundaries
-keep class com.anyplug.common.ui.DiscoveredServer { *; }
-keep class com.anyplug.common.ui.RemoteDevice { *; }
-keep class com.anyplug.common.ui.LocalUsbDevice { *; }

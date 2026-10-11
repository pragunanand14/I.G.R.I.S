# The plugin class and its commands are looked up by name from Rust.
-keep class dev.igris.device.DevicePlugin { *; }
-keep class dev.igris.device.LaunchArgs { *; }

# The offline wake-word recogniser is called through JNA (by name, from native code).
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class org.vosk.** { *; }
-dontwarn java.awt.**

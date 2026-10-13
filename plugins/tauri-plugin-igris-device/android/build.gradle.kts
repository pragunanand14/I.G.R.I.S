import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "dev.igris.device"
    compileSdk = 37

    defaultConfig {
        minSdk = 24
        consumerProguardFiles("consumer-rules.pro")
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_1_8
    }
}

dependencies {
    implementation("androidx.appcompat:appcompat:1.6.0")
    // "Hey IGRIS": an offline recogniser and its small English model (bundled; nothing is downloaded at run time).
    implementation("com.alphacephei:vosk-android:0.3.47")
    implementation("net.java.dev.jna:jna:5.13.0@aar")
    implementation("com.alphacephei:vosk-model-en:0.3.38")
    // Google sign-in with the account already on the phone (Google Play services' account picker).
    implementation("com.google.android.gms:play-services-auth:21.3.0")
    // Scanning documents with Google's scanner (edge finding, several pages, PDF); runs in Play services.
    implementation("com.google.android.gms:play-services-mlkit-document-scanner:16.0.0-beta1")
    implementation(project(":tauri-android"))
}

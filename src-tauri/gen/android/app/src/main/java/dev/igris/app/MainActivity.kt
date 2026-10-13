package dev.igris.app

import android.graphics.Color
import android.os.Build
import android.graphics.drawable.ColorDrawable
import android.os.Bundle
import android.view.View
import androidx.activity.enableEdgeToEdge
import androidx.core.graphics.Insets
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    // Drawn edge to edge, the keyboard would cover the bottom of the app (the chat box and the latest
    // messages). Shrink the app to the space above the keyboard while it's open instead.
    ViewCompat.setOnApplyWindowInsetsListener(findViewById<View>(android.R.id.content)) { v, insets ->
      v.setPadding(0, 0, 0, insets.getInsets(WindowInsetsCompat.Type.ime()).bottom)
      WindowInsetsCompat.Builder(insets).setInsets(WindowInsetsCompat.Type.ime(), Insets.NONE).build()
    }
    // The theme is see-through for the assistant panel; as a normal app IGRIS has a solid background
    // (the device plugin switches it for the panel).
    if (intent?.getStringExtra("dev.igris.action") != "assistant") {
      window.setBackgroundDrawable(ColorDrawable(Color.rgb(0x0C, 0x0C, 0x0E)))
      if (Build.VERSION.SDK_INT >= 30) setTranslucent(false)
    }
  }
}

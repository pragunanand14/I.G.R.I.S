package dev.igris.app

import android.graphics.Color
import android.os.Build
import android.graphics.drawable.ColorDrawable
import android.os.Bundle
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    // The theme is see-through for the assistant panel; as a normal app IGRIS has a solid background
    // (the device plugin switches it for the panel).
    if (intent?.getStringExtra("dev.igris.action") != "assistant") {
      window.setBackgroundDrawable(ColorDrawable(Color.rgb(0x0C, 0x0C, 0x0E)))
      if (Build.VERSION.SDK_INT >= 30) setTranslucent(false)
    }
  }
}

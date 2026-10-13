package dev.igris.device

import android.app.PendingIntent
import android.content.Intent
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService

/**
 * The IGRIS tile in Quick Settings (swipe down twice → edit → drag IGRIS in): one tap opens IGRIS's
 * assistant panel over whatever is on screen, listening — the same as saying "Hey IGRIS".
 */
class IgrisTileService : TileService() {
    override fun onStartListening() {
        super.onStartListening()
        qsTile?.apply {
            state = Tile.STATE_INACTIVE
            label = getString(R.string.igris_tile_label)
            updateTile()
        }
    }

    override fun onClick() {
        super.onClick()
        val open = packageManager.getLaunchIntentForPackage(packageName)?.apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_REORDER_TO_FRONT)
            putExtra(EXTRA_ACTION, ACTION_ASSISTANT)
        } ?: return
        if (Build.VERSION.SDK_INT >= 34) {
            startActivityAndCollapse(PendingIntent.getActivity(this, 7302, open, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT))
        } else {
            @Suppress("DEPRECATION", "StartActivityAndCollapseDeprecated")
            startActivityAndCollapse(open)
        }
    }
}

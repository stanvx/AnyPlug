package com.anyplug.discovery

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.os.Build
import android.util.Log

/**
 * mDNS advertiser for the Android USB/IP server.
 *
 * Uses Android's native [NsdManager] to advertise `_usbip._tcp`
 * with device metadata in TXT records, allowing Android TV, Windows,
 * Linux, and other AnyPlug clients on the LAN to auto-discover this server.
 */
class ServerAdvertiser(private val context: Context) {
    private val nsdManager = context.getSystemService(Context.NSD_SERVICE) as? NsdManager
    private var registrationListener: NsdManager.RegistrationListener? = null
    @Volatile
    private var isRegistered = false

    fun register(deviceName: String, vid: Int, pid: Int, port: Int = 3240) {
        if (isRegistered || nsdManager == null) return

        val hexVid = String.format("%04x", vid)
        val hexPid = String.format("%04x", pid)
        val sanitizedName = deviceName.replace(":", " ").replace(",", " ").trim()

        val serviceInfo = NsdServiceInfo().apply {
            serviceName = "AnyPlug-${Build.MODEL.take(16).trim()}"
            serviceType = "_usbip._tcp"
            setPort(port)
            setAttribute("devices", "$hexVid:$hexPid:1-1:$sanitizedName")
            setAttribute("vid", hexVid)
            setAttribute("pid", hexPid)
            setAttribute("bus", "1-1")
            setAttribute("name", sanitizedName)
            setAttribute("version", "1.1.1")
            setAttribute("platform", "Android")
        }

        val listener = object : NsdManager.RegistrationListener {
            override fun onServiceRegistered(info: NsdServiceInfo) {
                Log.i(TAG, "mDNS service registered: ${info.serviceName}")
                isRegistered = true
            }

            override fun onRegistrationFailed(info: NsdServiceInfo, errorCode: Int) {
                Log.e(TAG, "mDNS registration failed: errorCode=$errorCode")
                isRegistered = false
            }

            override fun onServiceUnregistered(info: NsdServiceInfo) {
                Log.i(TAG, "mDNS service unregistered: ${info.serviceName}")
                isRegistered = false
            }

            override fun onUnregistrationFailed(info: NsdServiceInfo, errorCode: Int) {
                Log.e(TAG, "mDNS unregistration failed: errorCode=$errorCode")
            }
        }

        registrationListener = listener
        try {
            nsdManager.registerService(serviceInfo, NsdManager.PROTOCOL_DNS_SD, listener)
        } catch (e: Exception) {
            Log.e(TAG, "Failed to register mDNS service with NsdManager", e)
        }
    }

    fun unregister() {
        val listener = registrationListener ?: return
        try {
            nsdManager?.unregisterService(listener)
        } catch (e: Exception) {
            Log.w(TAG, "Failed to unregister mDNS service", e)
        } finally {
            registrationListener = null
            isRegistered = false
        }
    }

    companion object {
        private const val TAG = "ServerAdvertiser"
    }
}

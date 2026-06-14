package com.anyplug.common.ui

/**
 * Shared data models used by both the phone and TV Compose UI modules.
 *
 * These were extracted from MainScreen.kt to allow the :common library
 * module to provide them without depending on the phone UI composables.
 */

data class DiscoveredServer(
    val host: String,
    val port: Int,
    val devices: List<RemoteDevice>
)

data class RemoteDevice(
    val name: String,
    val busId: String,
    val vid: Int,
    val pid: Int
)

data class LocalUsbDevice(
    val name: String,
    val vid: Int,
    val pid: Int
)

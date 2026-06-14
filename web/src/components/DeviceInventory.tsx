'use client';

import { useEffect, useState, useCallback } from 'react';
import { getDevices } from '@/lib/api';
import type { Device } from '@/lib/types';
import { RefreshCw, Monitor, Radio, Wifi, Zap } from 'lucide-react';

const SPEED_LABELS: Record<number, string> = {
  1: '1.5 Mbps (Low)',
  12: '12 Mbps (Full)',
  480: '480 Mbps (High)',
  5000: '5 Gbps (Super)',
  10000: '10 Gbps (Super+)',
};

export default function DeviceInventory() {
  const [devices, setDevices] = useState<Device[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);

  const fetchDevices = useCallback(async () => {
    try {
      setError(null);
      const data = await getDevices();
      setDevices(data);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
  }, []);

  useEffect(() => {
    fetchDevices();
    const interval = setInterval(fetchDevices, 10000);
    return () => clearInterval(interval);
  }, [fetchDevices]);

  function handleRefresh() {
    setRefreshing(true);
    fetchDevices();
  }

  function getStatusBadge(status: string) {
    const styles: Record<string, string> = {
      available: 'bg-[#2b9a5e]/20 text-[#2b9a5e] border-[#2b9a5e]/30',
      exported: 'bg-anyplug-600/20 text-anyplug-300 border-anyplug-600/30',
      error: 'bg-[#dc2626]/20 text-[#dc2626] border-[#dc2626]/30',
    };
    return styles[status] || 'bg-[#6b6f83]/20 text-[#6b6f83] border-[#6b6f83]/30';
  }

  function getSpeedIcon(speed: number) {
    if (speed >= 5000) return <Zap size={14} className="text-yellow-400" />;
    return <Wifi size={14} className="text-[#8b8fa3]" />;
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="animate-spin w-8 h-8 border-2 border-anyplug-500 border-t-transparent rounded-full" />
      </div>
    );
  }

  return (
    <div className="animate-fade-in">
      {/* Header */}
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-xl font-bold text-white">Device Inventory</h1>
          <p className="text-sm text-[#8b8fa3] mt-1">
            {devices.length} device{devices.length !== 1 ? 's' : ''} detected
          </p>
        </div>
        <button
          onClick={handleRefresh}
          disabled={refreshing}
          className="flex items-center gap-2 px-4 py-2 rounded-lg bg-[#1a1d28] border border-[#2a2e3a] text-sm text-[#8b8fa3] hover:text-white hover:border-anyplug-500/50 transition-colors disabled:opacity-50"
        >
          <RefreshCw size={14} className={refreshing ? 'animate-spin' : ''} />
          Refresh
        </button>
      </div>

      {error && (
        <div className="mb-4 p-4 rounded-lg bg-[#dc2626]/10 border border-[#dc2626]/20 text-sm text-[#dc2626]">
          {error}
        </div>
      )}

      {devices.length === 0 ? (
        <div className="flex flex-col items-center justify-center h-64 text-[#6b6f83]">
          <Monitor size={48} className="mb-4 opacity-50" />
          <p className="text-lg font-medium mb-1">No devices detected</p>
          <p className="text-sm">Plug in a USB device or check server permissions</p>
        </div>
      ) : (
        <div className="grid gap-3">
          {devices.map((device) => (
            <div
              key={device.busid}
              className="bg-[#1a1d28] border border-[#2a2e3a] rounded-xl p-5 hover:border-[#3a3e4a] transition-colors"
            >
              <div className="flex items-start justify-between">
                <div className="flex items-start gap-4">
                  <div className="w-12 h-12 rounded-lg bg-anyplug-600/20 flex items-center justify-center flex-shrink-0">
                    <Monitor size={22} className="text-anyplug-400" />
                  </div>
                  <div>
                    <h3 className="text-white font-semibold text-base mb-1">
                      {device.path}
                    </h3>
                    <div className="flex items-center gap-3 text-xs text-[#8b8fa3]">
                      <span>VID:{device.vid.toString(16).padStart(4, '0')}</span>
                      <span>PID:{device.pid.toString(16).padStart(4, '0')}</span>
                      <span className="flex items-center gap-1">
                        {getSpeedIcon(device.speed)}
                        {SPEED_LABELS[device.speed] || `${device.speed} Mbps`}
                      </span>
                    </div>
                  </div>
                </div>
                <div className="flex items-center gap-3">
                  {device.connected_client && (
                    <span className="flex items-center gap-1 text-xs text-anyplug-400">
                      <Radio size={12} />
                      {device.connected_client}
                    </span>
                  )}
                  <span
                    className={`px-3 py-1 rounded-full text-xs font-medium border ${getStatusBadge(device.status)}`}
                  >
                    {device.status}
                  </span>
                </div>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

'use client';

import { useEffect, useState, useCallback } from 'react';
import { getStatus } from '@/lib/api';
import type { ServerStatus } from '@/lib/types';
import { useWebSocket } from '@/hooks/useWebSocket';
import {
  Heart,
  Clock,
  Radio,
  Monitor,
  HardDrive,
  AlertTriangle,
  Activity,
} from 'lucide-react';

export default function ServerHealth() {
  const { isConnected: wsConnected, lastEvent } = useWebSocket();
  const [status, setStatus] = useState<ServerStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchStatus = useCallback(async () => {
    try {
      setError(null);
      const data = await getStatus();
      setStatus(data);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchStatus();
    const interval = setInterval(fetchStatus, 5000);
    return () => clearInterval(interval);
  }, [fetchStatus]);

  function formatUptime(secs: number): string {
    const d = Math.floor(secs / 86400);
    const h = Math.floor((secs % 86400) / 3600);
    const m = Math.floor((secs % 3600) / 60);
    const s = Math.floor(secs % 60);
    const parts: string[] = [];
    if (d > 0) parts.push(`${d}d`);
    if (h > 0) parts.push(`${h}h`);
    if (m > 0) parts.push(`${m}m`);
    parts.push(`${s}s`);
    return parts.join(' ');
  }

  function formatMemory(bytes: number): string {
    if (bytes >= 1073741824) return `${(bytes / 1073741824).toFixed(1)} GB`;
    if (bytes >= 1048576) return `${(bytes / 1048576).toFixed(1)} MB`;
    if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${bytes} B`;
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
      <div className="mb-6">
        <h1 className="text-xl font-bold text-white">Server Health</h1>
        <p className="text-sm text-[#8b8fa3] mt-1">
          Real-time server status and resource monitoring
        </p>
      </div>

      {error && (
        <div className="mb-4 p-4 rounded-lg bg-[#dc2626]/10 border border-[#dc2626]/20 text-sm text-[#dc2626]">
          {error}
        </div>
      )}

      {/* Status banner */}
      {status && (
        <div
          className={`mb-6 p-4 rounded-xl border ${
            status.status === 'running'
              ? 'bg-[#2b9a5e]/10 border-[#2b9a5e]/20'
              : 'bg-yellow-400/10 border-yellow-400/20'
          }`}
        >
          <div className="flex items-center gap-3">
            <div
              className={`w-3 h-3 rounded-full ${
                status.status === 'running'
                  ? 'bg-[#2b9a5e] animate-pulse-dot'
                  : 'bg-yellow-400'
              }`}
            />
            <div>
              <span className="text-white font-medium text-sm">
                Server {status.status}
              </span>
              <span className="text-[#8b8fa3] text-xs ml-3">
                v{status.version}
              </span>
            </div>
          </div>
        </div>
      )}

      {/* Metrics grid */}
      {status && (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4 mb-6">
          <MetricCard
            icon={<Clock size={18} className="text-anyplug-400" />}
            label="Uptime"
            value={formatUptime(status.uptime_secs || status.uptime)}
          />
          <MetricCard
            icon={<Radio size={18} className="text-[#2b9a5e]" />}
            label="Active Connections"
            value={String(status.active_connections)}
          />
          <MetricCard
            icon={<Monitor size={18} className="text-anyplug-400" />}
            label="Devices"
            value={String(status.devices_count)}
          />
          <MetricCard
            icon={<Activity size={18} className="text-yellow-400" />}
            label="WebSocket"
            value={wsConnected ? 'Connected' : 'Disconnected'}
            valueColor={wsConnected ? 'text-[#2b9a5e]' : 'text-[#dc2626]'}
          />
          <MetricCard
            icon={<HardDrive size={18} className="text-[#8b8fa3]" />}
            label="Memory (RSS)"
            value={formatMemory(status.memory_usage)}
          />
          <MetricCard
            icon={<AlertTriangle size={18} className={status.error_count > 0 ? 'text-yellow-400' : 'text-[#8b8fa3]'} />}
            label="Errors"
            value={String(status.error_count)}
            valueColor={status.error_count > 0 ? 'text-yellow-400' : ''}
          />
        </div>
      )}

      {/* REST API info */}
      <div className="bg-[#1a1d28] border border-[#2a2e3a] rounded-xl p-5">
        <h3 className="text-sm font-semibold text-white mb-3">API Endpoints</h3>
        <div className="space-y-2 text-sm">
          <ApiEndpoint method="GET" path="/api/status" desc="Server health" />
          <ApiEndpoint method="GET" path="/api/devices" desc="Device list" />
          <ApiEndpoint method="GET" path="/api/config" desc="Server config" />
          <ApiEndpoint method="POST" path="/api/connect" desc="Connect device" />
          <ApiEndpoint method="POST" path="/api/disconnect" desc="Disconnect device" />
          <ApiEndpoint method="WS" path="/api/events" desc="Real-time events" />
        </div>
      </div>
    </div>
  );
}

// ── Sub-components ────────────────────────────────────────────

function MetricCard({
  icon,
  label,
  value,
  valueColor,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  valueColor?: string;
}) {
  return (
    <div className="bg-[#1a1d28] border border-[#2a2e3a] rounded-xl p-5">
      <div className="flex items-center gap-2 mb-3">
        {icon}
        <span className="text-xs text-[#8b8fa3]">{label}</span>
      </div>
      <div className={`text-lg font-bold ${valueColor || 'text-white'}`}>
        {value}
      </div>
    </div>
  );
}

function ApiEndpoint({
  method,
  path,
  desc,
}: {
  method: string;
  path: string;
  desc: string;
}) {
  const methodColors: Record<string, string> = {
    GET: 'text-[#2b9a5e]',
    POST: 'text-anyplug-400',
    WS: 'text-yellow-400',
  };
  return (
    <div className="flex items-center gap-3 py-1.5 px-3 rounded-lg bg-[#0f1117]/50">
      <span className={`text-xs font-mono font-bold w-10 ${methodColors[method] || 'text-[#8b8fa3]'}`}>
        {method}
      </span>
      <code className="text-xs text-[#8b8fa3] font-mono flex-1">{path}</code>
      <span className="text-xs text-[#6b6f83]">{desc}</span>
    </div>
  );
}

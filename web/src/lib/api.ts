import type { Device, ServerStatus, ServerConfig, ConnectRequest, DisconnectRequest, ScanResult, DiscoveredServer, RemoteDevice } from './types';

const LS_KEY = 'anyplug_api_url';

export function getApiBase(): string {
  if (typeof window === 'undefined') {
    return process.env.NEXT_PUBLIC_API_BASE || '';
  }
  return process.env.NEXT_PUBLIC_API_BASE || localStorage.getItem(LS_KEY) || '';
}

export function setApiBase(url: string): void {
  localStorage.setItem(LS_KEY, url);
}

async function apiFetch<T>(path: string, options?: RequestInit): Promise<T> {
  const base = getApiBase();
  const res = await fetch(`${base}${path}`, {
    headers: { 'Content-Type': 'application/json', ...options?.headers },
    ...options,
  });
  if (!res.ok) {
    const body = await res.text();
    throw new Error(`API ${res.status}: ${body}`);
  }
  return res.json();
}

export async function getStatus(): Promise<ServerStatus> {
  return apiFetch<ServerStatus>('/api/status');
}

export async function getDevices(): Promise<Device[]> {
  return apiFetch<Device[]>('/api/devices');
}

export async function getConfig(): Promise<ServerConfig> {
  return apiFetch<ServerConfig>('/api/config');
}

export async function updateConfig(config: Partial<ServerConfig>): Promise<ServerConfig> {
  return apiFetch<ServerConfig>('/api/config', {
    method: 'PUT',
    body: JSON.stringify(config),
  });
}

export async function scanServers(): Promise<ScanResult> {
  const data = await apiFetch<ScanResult | DiscoveredServer[]>('/api/scan', {
    method: 'POST',
    body: JSON.stringify({ timeout_secs: 5 }),
  });

  if (Array.isArray(data)) {
    const devices: RemoteDevice[] = [];
    for (const server of data) {
      const devStr = server.txt?.devices;
      let added = false;
      if (devStr) {
        if (devStr.includes('vid=')) {
          const tokens = devStr.split(',');
          let curVid = 0;
          let curPid = 0;
          let curBus = '1-1';
          let curName = 'USB Device';
          for (const token of tokens) {
            const [k, v] = token.split('=').map((s) => s.trim());
            if (k === 'vid') {
              if (curVid && curPid) {
                devices.push({ host: server.host, port: server.port, busid: curBus, vid: curVid, pid: curPid, path: curName });
                added = true;
                curBus = '1-1';
                curName = 'USB Device';
              }
              curVid = parseInt(v?.replace(/^0x/, '') || '0', 16) || 0;
            } else if (k === 'pid') {
              curPid = parseInt(v?.replace(/^0x/, '') || '0', 16) || 0;
            } else if (k === 'bus') {
              curBus = v || '1-1';
            } else if (k === 'n') {
              curName = v || 'USB Device';
            }
          }
          if (curVid && curPid) {
            devices.push({ host: server.host, port: server.port, busid: curBus, vid: curVid, pid: curPid, path: curName });
            added = true;
          }
        } else {
          for (const entry of devStr.split(',')) {
            const parts = entry.split(':');
            if (parts.length >= 4) {
              const vid = parseInt(parts[0], 16) || 0;
              const pid = parseInt(parts[1], 16) || 0;
              devices.push({ host: server.host, port: server.port, busid: parts[2], vid, pid, path: parts[3] });
              added = true;
            }
          }
        }
      }
      if (!added) {
        devices.push({ host: server.host, port: server.port, busid: '1-1', vid: 0, pid: 0, path: 'USB Device' });
      }
    }
    return { devices };
  }

  return data;
}

export async function connectDevice(req: ConnectRequest): Promise<void> {
  await apiFetch<void>('/api/connect', {
    method: 'POST',
    body: JSON.stringify(req),
  });
}

export async function disconnectDevice(req: DisconnectRequest): Promise<void> {
  await apiFetch<void>('/api/disconnect', {
    method: 'POST',
    body: JSON.stringify(req),
  });
}

export function connectEventsWebSocket(): WebSocket {
  const base = getApiBase();
  const host = base.replace(/^https?:\/\//, '');
  const protocol = base.startsWith('https') ? 'wss:' : 'ws:';
  return new WebSocket(`${protocol}//${host}/api/events`);
}

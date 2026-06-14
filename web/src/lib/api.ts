import { Device, ServerStatus, ServerConfig, ConnectRequest, DisconnectRequest } from './types';

// Default to the same host:port the page was loaded from
const API_BASE = '';

async function apiFetch<T>(path: string, options?: RequestInit): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
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
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const host = window.location.host;
  return new WebSocket(`${protocol}//${host}/api/events`);
}

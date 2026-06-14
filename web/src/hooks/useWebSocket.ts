'use client';

import { useEffect, useRef, useState, useCallback } from 'react';
import { connectEventsWebSocket } from '@/lib/api';
import type { WsEvent } from '@/lib/types';

type WsEventHandler = (event: WsEvent) => void;

interface UseWebSocketResult {
  isConnected: boolean;
  lastEvent: WsEvent | null;
  subscribe: (handler: WsEventHandler) => () => void;
  error: string | null;
}

export function useWebSocket(): UseWebSocketResult {
  const wsRef = useRef<WebSocket | null>(null);
  const handlersRef = useRef<Set<WsEventHandler>>(new Set());
  const [isConnected, setIsConnected] = useState(false);
  const [lastEvent, setLastEvent] = useState<WsEvent | null>(null);
  const [error, setError] = useState<string | null>(null);
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const subscribe = useCallback((handler: WsEventHandler): (() => void) => {
    handlersRef.current.add(handler);
    return () => {
      handlersRef.current.delete(handler);
    };
  }, []);

  useEffect(() => {
    let mounted = true;

    function connect() {
      if (!mounted) return;
      try {
        const ws = connectEventsWebSocket();
        wsRef.current = ws;

        ws.onopen = () => {
          if (mounted) {
            setIsConnected(true);
            setError(null);
          }
        };

        ws.onmessage = (msg) => {
          if (!mounted) return;
          try {
            const event = JSON.parse(msg.data) as WsEvent;
            setLastEvent(event);
            handlersRef.current.forEach((h) => h(event));
          } catch {
            // ignore malformed messages
          }
        };

        ws.onclose = () => {
          if (mounted) {
            setIsConnected(false);
            // Reconnect after 3 seconds
            reconnectTimerRef.current = setTimeout(connect, 3000);
          }
        };

        ws.onerror = () => {
          if (mounted) {
            setError('WebSocket connection error');
          }
        };
      } catch (e) {
        if (mounted) {
          setError(String(e));
          reconnectTimerRef.current = setTimeout(connect, 5000);
        }
      }
    }

    connect();

    return () => {
      mounted = false;
      if (reconnectTimerRef.current) {
        clearTimeout(reconnectTimerRef.current);
      }
      if (wsRef.current) {
        wsRef.current.close();
      }
    };
  }, []);

  return { isConnected, lastEvent, subscribe, error };
}

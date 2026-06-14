'use client';

import { ReactNode } from 'react';
import './globals.css';
import AppLayout from '@/components/Layout';
import { useWebSocket } from '@/hooks/useWebSocket';

export default function RootLayout({ children }: { children: ReactNode }) {
  const { isConnected } = useWebSocket();

  return (
    <html lang="en">
      <body>
        <AppLayout wsConnected={isConnected}>{children}</AppLayout>
      </body>
    </html>
  );
}

import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '../index.css';
import { BridgeApp } from './BridgeApp';

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <BridgeApp />
  </StrictMode>,
);

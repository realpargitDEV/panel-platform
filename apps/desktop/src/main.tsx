import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import App from './App';
import './styles.css';

const container = document.getElementById('root');
if (!container) {
  // Cannot happen with the shipped index.html, but failing loudly beats a
  // blank window that gives no clue what went wrong.
  throw new Error('the root element is missing from index.html');
}

createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

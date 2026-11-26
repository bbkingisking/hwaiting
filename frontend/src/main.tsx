import React from 'react'
import ReactDOM from 'react-dom/client'
import { setWasmUrl } from '@lottiefiles/dotlottie-react'
import playerWasmUrl from '@lottiefiles/dotlottie-web/dotlottie-player.wasm?url'
import App from './App.tsx'
import './index.css'

// Override the player’s CDN default before any animation is mounted.
setWasmUrl(playerWasmUrl)

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
)

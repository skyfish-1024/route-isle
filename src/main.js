import { createApp } from 'vue'
import 'virtual:uno.css'
import './motion.css'
import App from './App.vue'

if (typeof document !== 'undefined') {
  document.addEventListener(
    'contextmenu',
    (event) => {
      const target =
        event.target instanceof Element ? event.target : event.target?.parentElement || null
      if (target?.closest('input, textarea, [contenteditable="true"]')) return
      event.preventDefault()
      event.stopPropagation()
    },
    { capture: true },
  )
}

createApp(App).mount('#app')

import './style.css'
import { initEditor } from './editor'
import { initSettings } from './settings'

const view = new URL(window.location.href).searchParams.get('view')

if (view === 'settings') {
  void initSettings()
} else {
  void initEditor()
}

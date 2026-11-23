const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('ocr', {
  recognizeLines: (lines, withEnglish) => ipcRenderer.invoke('ocr-lines', { lines, withEnglish }),
  copy: (text) => ipcRenderer.invoke('copy-text', text),
  saveDocx: (opts) => ipcRenderer.invoke('save-docx', opts),
  saveTxt: (opts) => ipcRenderer.invoke('save-txt', opts),
  openFiles: () => ipcRenderer.invoke('open-files'),
  readWords: () => ipcRenderer.invoke('read-words'),
  labelSave: (o) => ipcRenderer.invoke('label-save', o),
  labelUndo: (id) => ipcRenderer.invoke('label-undo', id),
  labelShow: () => ipcRenderer.invoke('label-show'),
  labelCount: () => ipcRenderer.invoke('label-count'),
  info: () => ipcRenderer.invoke('app-info'),
  onProgress: (cb) => ipcRenderer.on('ocr-progress', (_e, p) => cb(p)),
});

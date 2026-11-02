import { useEffect, useRef, type FormEvent } from 'react'
import { FolderOpen, X } from 'lucide-react'
type Props = { path: string; error: string; busy: boolean; onPath: (path: string) => void; onClose: () => void; onSubmit: (event: FormEvent<HTMLFormElement>) => void }
export default function ConnectDirectory({ path, error, busy, onPath, onClose, onSubmit }: Props) {
  const dialog = useRef<HTMLDialogElement>(null)
  const pathInput = useRef<HTMLInputElement>(null)
  useEffect(() => {
    const element = dialog.current
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null
    element?.showModal()
    pathInput.current?.focus()
    return () => { element?.close(); if (opener?.isConnected) opener.focus() }
  }, [])
  return <dialog ref={dialog} className="video-backdrop" aria-labelledby="connect-directory-title" onCancel={event => { event.preventDefault(); onClose() }} onClick={event => { if (event.target === event.currentTarget) onClose() }}>
    <form className="connect-modal" onSubmit={onSubmit}>
      <button className="modal-close" type="button" aria-label="Close connect directory" onClick={onClose}><X size={17}/></button>
      <span className="modal-icon"><FolderOpen size={19}/></span><div className="section-kicker">ADD A CONTENT SOURCE</div>
      <h2 id="connect-directory-title">Connect a directory</h2><p>Choose an existing folder on the computer running Grimoire. We’ll scan its subfolders for supported media.</p>
      <label htmlFor="directory-path">Directory path</label><input ref={pathInput} id="directory-path" value={path} onChange={event => onPath(event.target.value)} placeholder="/home/you/Media" required/>
      <small className="path-hint">Use a full local path, such as <code>/home/you/Videos</code>.</small>
      {error && <div className="connect-error" role="alert">{error}</div>}
      <button className="connect-submit" type="submit" disabled={busy}>{busy ? 'Connecting…' : 'Connect directory'}</button>
      <span className="modal-note">Disconnecting a source never deletes its files.</span>
    </form>
  </dialog>
}

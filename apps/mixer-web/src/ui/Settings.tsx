import { Globe, LockSimple, X } from '@phosphor-icons/react';
import { useEffect, useState } from 'react';
import { getAccess, updateAccess, type Access } from '../lib/access';
import { useMixer } from '../lib/store';
import { NewPassword } from './PasswordField';
import { Btn, cx } from './kit';

type Editing = 'remote' | 'app' | null;

/** Configurações de acesso: acesso remoto e senha do app. */
export function Settings({ onClose }: { onClose: () => void }) {
  const toast = useMixer((s) => s.toast);
  const [acc, setAcc] = useState<Access | null>(null);
  const [editing, setEditing] = useState<Editing>(null);
  const [pw, setPw] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const reload = () => getAccess().then(setAcc).catch(() => {});
  useEffect(() => {
    reload();
    const esc = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    window.addEventListener('keydown', esc);
    return () => window.removeEventListener('keydown', esc);
  }, [onClose]);

  const save = async (change: Parameters<typeof updateAccess>[0], msg: string) => {
    setBusy(true);
    try {
      await updateAccess(change);
      toast('info', msg);
      setEditing(null);
      setPw(null);
      await reload();
    } catch (e) {
      toast('error', (e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const editor = (which: Exclude<Editing, null>) =>
    editing === which && (
      <form
        className="flex flex-col gap-2 rounded-lg bg-bg/60 p-3"
        onSubmit={(e) => {
          e.preventDefault();
          if (!pw) return;
          if (which === 'remote') save({ remote_password: pw }, acc?.remote_enabled ? 'Senha remota trocada. Quem entrou com a antiga precisa entrar de novo.' : 'Acesso remoto ligado.');
          else save({ app_password: pw }, 'Senha do app salva.');
        }}
      >
        <NewPassword label={which === 'remote' ? 'Nova senha de acesso remoto' : 'Nova senha do app'} onChange={setPw} autoFocus />
        <div className="flex justify-end gap-2">
          <Btn variant="ghost" onClick={() => setEditing(null)}>Cancelar</Btn>
          <Btn variant="primary" type="submit" disabled={!pw || busy}>Salvar</Btn>
        </div>
      </form>
    );

  return (
    <div className="fixed inset-0 z-40 grid place-items-center bg-deep/70 p-4 backdrop-blur-sm" onPointerDown={(e) => e.target === e.currentTarget && onClose()}>
      <div role="dialog" aria-modal="true" aria-label="Configurações" className="flex max-h-full w-full max-w-lg flex-col gap-4 overflow-auto rounded-[18px] border border-line bg-panel p-6 shadow-[var(--shadow-modal)]">
        <header className="flex items-center justify-between">
          <h2 className="text-[16px] font-bold text-hi">Configurações</h2>
          <button type="button" aria-label="Fechar" onClick={onClose} className="grid h-8 w-8 place-items-center rounded-lg text-subtle hover:bg-panel-2 hover:text-text">
            <X size={16} weight="bold" />
          </button>
        </header>

        <section className="flex flex-col gap-3 rounded-[14px] border border-line bg-panel-2/60 p-4">
          <div className="flex items-center gap-3">
            <Globe size={20} weight="bold" className="shrink-0 text-cyan" />
            <h3 className="flex-1 text-[13.5px] font-bold text-hi">Acesso de outros computadores</h3>
            <span className={cx('rounded-full px-2 py-0.5 text-[11px] font-bold', acc?.remote_enabled ? 'bg-ok/15 text-ok-light' : 'bg-panel text-muted')}>{acc?.remote_enabled ? 'LIGADO' : 'DESLIGADO'}</span>
          </div>
          {acc?.remote_enabled ? (
            <>
              <p className="text-[12px] text-subtle">Na rede da igreja, abra um destes endereços e entre com a senha de acesso remoto:</p>
              {acc.lan_urls.length ? (
                acc.lan_urls.map((u) => <input key={u} readOnly value={u} aria-label="Endereço na rede" onFocus={(e) => e.target.select()} className="lm-field h-9 px-3 font-mono text-[12.5px] text-hi" />)
              ) : (
                <p className="text-[12px] text-warn-light">Este computador não está numa rede agora.</p>
              )}
            </>
          ) : (
            <p className="text-[12px] text-subtle">Só este computador acessa o console. Crie uma senha para liberar notebooks e tablets da rede.</p>
          )}
          {acc?.local === false && <p className="text-[11.5px] text-muted">Você está num acesso remoto. Se desligar o acesso remoto, esta tela também sai.</p>}
          {editor('remote')}
          {editing !== 'remote' && (
            <div className="flex justify-end gap-2">
              {acc?.remote_enabled && (
                <Btn variant="ghost" disabled={busy} onClick={() => confirm('Desligar o acesso remoto? Todos os computadores da rede saem do console. O áudio continua.') && save({ remote_password: null }, 'Acesso remoto desligado.')}>
                  Desligar
                </Btn>
              )}
              <Btn variant={acc?.remote_enabled ? 'secondary' : 'primary'} onClick={() => setEditing('remote')}>
                {acc?.remote_enabled ? 'Trocar senha' : 'Ligar com senha'}
              </Btn>
            </div>
          )}
        </section>

        <section className="flex flex-col gap-3 rounded-[14px] border border-line bg-panel-2/60 p-4">
          <div className="flex items-center gap-3">
            <LockSimple size={20} weight="bold" className="shrink-0 text-accent-text" />
            <h3 className="flex-1 text-[13.5px] font-bold text-hi">Senha neste computador</h3>
            <span className={cx('rounded-full px-2 py-0.5 text-[11px] font-bold', acc?.app_password ? 'bg-ok/15 text-ok-light' : 'bg-panel text-muted')}>{acc?.app_password ? 'COM SENHA' : 'SEM SENHA'}</span>
          </div>
          <p className="text-[12px] text-subtle">{acc?.app_password ? 'O console pede a senha do app para abrir no computador do servidor.' : 'O console abre direto no computador do servidor.'}</p>
          {editor('app')}
          {editing !== 'app' && (
            <div className="flex justify-end gap-2">
              {acc?.app_password && (
                <Btn variant="ghost" disabled={busy} onClick={() => confirm('Tirar a senha deste computador?') && save({ app_password: null }, 'Senha do app removida.')}>
                  Remover
                </Btn>
              )}
              <Btn variant={acc?.app_password ? 'secondary' : 'primary'} onClick={() => setEditing('app')}>
                {acc?.app_password ? 'Trocar senha' : 'Criar senha'}
              </Btn>
            </div>
          )}
        </section>

        <p className="text-[11px] leading-relaxed text-muted">
          Esqueceu uma senha? No computador do servidor: <code className="font-mono text-subtle">sona-server password --app --clear</code> ou <code className="font-mono text-subtle">--remote --clear</code>.
        </p>
      </div>
    </div>
  );
}

import {
  ArrowDown,
  ArrowUp,
  Headphones,
  Pause,
  PencilSimple,
  Play,
  Plus,
  SkipBack,
  SkipForward,
  Stop,
  Trash,
  UploadSimple,
  X,
} from '@phosphor-icons/react';
import { useEffect, useRef, useState } from 'react';
import { fmtDb, fmtTime, isAudioFile, playerApi, type PlayerSong, type PlayerState, type PlayerTrack } from '../lib/player';
import { useMixer } from '../lib/store';
import { Btn, Card, Switch, Toggle, cx } from './kit';

interface Upload {
  label: string;
  done: number;
  total: number;
  sent: number;
  bytes: number;
}

/** Pasta comum dos arquivos arrastados/escolhidos (vira o nome da música). */
function folderName(files: File[]): string | null {
  const p = (files[0] as File & { webkitRelativePath?: string }).webkitRelativePath;
  return p && p.includes('/') ? p.split('/')[0] : null;
}

/** Arquivos de uma pasta arrastada (DataTransfer com diretórios). */
async function filesFromDrop(dt: DataTransfer): Promise<{ files: File[]; folder: string | null }> {
  const items = Array.from(dt.items ?? []);
  const entries = items.map((i) => (i as DataTransferItem & { webkitGetAsEntry?: () => FileSystemEntry | null }).webkitGetAsEntry?.()).filter(Boolean) as FileSystemEntry[];
  if (!entries.length) return { files: Array.from(dt.files), folder: null };
  const out: File[] = [];
  let folder: string | null = null;
  const walk = async (e: FileSystemEntry): Promise<void> => {
    if (e.isFile) {
      out.push(await new Promise<File>((res, rej) => (e as FileSystemFileEntry).file(res, rej)));
    } else if (e.isDirectory) {
      const reader = (e as FileSystemDirectoryEntry).createReader();
      for (;;) {
        const batch = await new Promise<FileSystemEntry[]>((res, rej) => reader.readEntries(res, rej));
        if (!batch.length) break;
        for (const c of batch) await walk(c);
      }
    }
  };
  if (entries.length === 1 && entries[0].isDirectory) folder = entries[0].name;
  for (const e of entries) await walk(e);
  return { files: out, folder };
}

export function Player() {
  const player = useMixer((s) => s.player);
  const setPlayer = useMixer((s) => s.setPlayer);
  const toast = useMixer((s) => s.toast);
  const [upload, setUpload] = useState<Upload | null>(null);

  if (!player) return <div className="grid flex-1 place-items-center text-subtle">Carregando o player…</div>;

  const run = async (p: Promise<unknown>) => {
    try {
      const r = await p;
      if (r && typeof r === 'object' && 'songs' in r) setPlayer(r as PlayerState);
    } catch (e) {
      toast('error', (e as Error).message);
    }
  };

  const sendFiles = async (songId: string, list: File[], label: string) => {
    const files = list.filter((f) => isAudioFile(f.name));
    const skipped = list.length - files.length;
    if (!files.length) return toast('error', 'Nenhum arquivo de áudio (WAV, AIFF, FLAC ou MP3).');
    const bytes = files.reduce((a, f) => a + f.size, 0);
    let base = 0;
    setUpload({ label, done: 0, total: files.length, sent: 0, bytes });
    const errors: string[] = [];
    for (const [i, f] of files.entries()) {
      try {
        await playerApi.upload(songId, f, (s) => setUpload({ label, done: i, total: files.length, sent: base + s, bytes }));
      } catch (e) {
        errors.push((e as Error).message);
      }
      base += f.size;
      setUpload({ label, done: i + 1, total: files.length, sent: base, bytes });
    }
    setUpload(null);
    if (errors.length) toast('error', errors[0] + (errors.length > 1 ? ` (+${errors.length - 1})` : ''));
    else toast('info', `${files.length} faixa(s) em “${label}”.${skipped ? ` ${skipped} arquivo(s) ignorado(s).` : ''}`);
  };

  const newSong = async (files: File[], folder: string | null) => {
    const suggested = folder ?? folderName(files) ?? 'Nova música';
    const name = prompt('Nome da música', suggested);
    if (!name) return;
    try {
      const { id } = await playerApi.create(name);
      await sendFiles(id, files, name);
    } catch (e) {
      toast('error', (e as Error).message);
    }
  };

  const song = player.songs.find((s) => s.id === player.current) ?? null;

  return (
    <div className="flex min-h-0 flex-1 flex-wrap content-start items-start gap-4 overflow-auto p-4">
      <aside className="flex w-full flex-col gap-3 lg:w-[280px]">
        <Setlist player={player} run={run} />
        <DropZone onFiles={newSong} upload={upload} />
      </aside>
      <main className="flex min-w-0 flex-[999_1_560px] flex-col gap-4">
        {song ? (
          <>
            <Transport player={player} song={song} run={run} onAdd={(files) => sendFiles(song.id, files, song.name)} />
            <Tracks player={player} song={song} run={run} />
          </>
        ) : (
          <Card title="Player multitrack">
            <p className="text-[13px] leading-relaxed text-subtle">
              Carregue as faixas separadas de uma música (WAV, AIFF, FLAC ou MP3). Elas ficam guardadas neste computador e tocam pela mesa do Sona: somadas num canal “Player” ou cada uma no seu canal. Click e guia vão só para o fone.
            </p>
          </Card>
        )}
      </main>
      <aside className="flex w-full flex-col gap-3 lg:w-[300px]">
        <DeskSummary player={player} song={song} />
        <Card title="Volume geral do player">
          <GainSlider label="Volume geral do player" value={player.master_db} onCommit={(v) => run(playerApi.action('settings', { master_db: v }))} />
        </Card>
        {player.errors.length > 0 && (
          <Card title="Faixas com problema">
            {player.errors.map((e) => (
              <p key={e} className="text-[12px] text-warn-light">{e}</p>
            ))}
          </Card>
        )}
      </aside>
    </div>
  );
}

function Setlist({ player, run }: { player: PlayerState; run: (p: Promise<unknown>) => void }) {
  const ids = player.songs.map((s) => s.id);
  const move = (i: number, d: number) => {
    const next = [...ids];
    const [x] = next.splice(i, 1);
    next.splice(i + d, 0, x);
    run(playerApi.action('reorder', { ids: next }));
  };
  return (
    <Card title="Repertório" right={<span className="font-mono text-[11px] text-muted">{player.songs.length} músicas</span>}>
      {player.songs.length === 0 && <p className="text-[12px] text-muted">Nenhuma música ainda.</p>}
      <ol className="flex flex-col gap-1.5">
        {player.songs.map((s, i) => {
          const cur = s.id === player.current;
          return (
            <li key={s.id} className={cx('group flex items-center gap-1 rounded-[10px] border', cur ? 'border-accent bg-accent/15' : 'border-line bg-panel-2')}>
              <button type="button" onClick={() => run(playerApi.action('load', { id: s.id }))} aria-current={cur} className="flex min-h-12 min-w-0 flex-1 items-center gap-2.5 px-2.5 py-1.5 text-left">
                <span className="w-4 font-mono text-[11px] text-muted">{i + 1}</span>
                <span className="flex min-w-0 flex-1 flex-col">
                  <span className="truncate text-[13px] font-bold text-text">{s.name}</span>
                  <span className="text-[11px] text-muted">
                    {[s.bpm ? `${s.bpm} BPM` : null, s.key || null, `${s.tracks.length} faixas`].filter(Boolean).join(' · ')}
                  </span>
                </span>
                <span className="font-mono text-[11px] text-subtle">{fmtTime(s.seconds)}</span>
              </button>
              <span className="flex flex-col pr-1 opacity-60 group-hover:opacity-100">
                <button type="button" aria-label={`Subir ${s.name}`} disabled={i === 0} onClick={() => move(i, -1)} className="grid h-5 w-6 place-items-center text-subtle hover:text-text disabled:opacity-30">
                  <ArrowUp size={12} weight="bold" />
                </button>
                <button type="button" aria-label={`Descer ${s.name}`} disabled={i === ids.length - 1} onClick={() => move(i, 1)} className="grid h-5 w-6 place-items-center text-subtle hover:text-text disabled:opacity-30">
                  <ArrowDown size={12} weight="bold" />
                </button>
              </span>
            </li>
          );
        })}
      </ol>
      <label className="flex items-center gap-2 pt-1 text-[12px] text-subtle">
        <Switch label="Parar no fim de cada música" on={player.stop_at_end} onChange={(v) => run(playerApi.action('settings', { stop_at_end: v }))} />
        Parar no fim de cada música
      </label>
    </Card>
  );
}

function DropZone({ onFiles, upload }: { onFiles: (files: File[], folder: string | null) => void; upload: Upload | null }) {
  const [over, setOver] = useState(false);
  const filesRef = useRef<HTMLInputElement>(null);
  const dirRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    dirRef.current?.setAttribute('webkitdirectory', '');
  }, []);
  const pick = (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = Array.from(e.target.files ?? []);
    e.target.value = '';
    if (files.length) onFiles(files, null);
  };
  if (upload) {
    const pct = upload.bytes ? Math.round((upload.sent / upload.bytes) * 100) : 0;
    return (
      <div className="flex flex-col gap-2 rounded-[14px] border border-accent/60 bg-accent/10 p-4" role="status">
        <span className="text-[13px] font-bold text-text">Enviando “{upload.label}”</span>
        <div className="h-2 overflow-hidden rounded-full bg-deep">
          <div className="h-full bg-accent transition-[width]" style={{ width: `${pct}%` }} />
        </div>
        <span className="font-mono text-[11.5px] text-subtle">
          {upload.done}/{upload.total} faixas · {pct}% · {(upload.bytes / 1e6).toFixed(0)} MB
        </span>
      </div>
    );
  }
  return (
    <div
      onDragOver={(e) => {
        e.preventDefault();
        setOver(true);
      }}
      onDragLeave={() => setOver(false)}
      onDrop={async (e) => {
        e.preventDefault();
        setOver(false);
        const { files, folder } = await filesFromDrop(e.dataTransfer);
        if (files.length) onFiles(files, folder);
      }}
      className={cx('flex flex-col items-center gap-2 rounded-[14px] border-[1.5px] border-dashed p-4 text-center transition', over ? 'border-accent bg-accent/10' : 'border-line-hi')}
    >
      <UploadSimple size={26} weight="bold" className="text-accent-light" />
      <span className="text-[13px] font-bold text-text">Nova música</span>
      <span className="text-[12px] leading-snug text-subtle">Arraste a pasta da música ou os arquivos das faixas. Eles vão para o computador do Sona e ficam guardados.</span>
      <div className="mt-1 flex flex-wrap justify-center gap-2">
        <Btn onClick={() => dirRef.current?.click()}>Escolher pasta</Btn>
        <Btn onClick={() => filesRef.current?.click()}>Escolher arquivos</Btn>
      </div>
      <input ref={filesRef} type="file" multiple accept=".wav,.wave,.aif,.aiff,.flac,.mp3,audio/*" className="hidden" onChange={pick} />
      <input ref={dirRef} type="file" multiple className="hidden" onChange={pick} />
    </div>
  );
}

function Transport({ player, song, run, onAdd }: { player: PlayerState; song: PlayerSong; run: (p: Promise<unknown>) => void; onAdd: (f: File[]) => void }) {
  const tick = useMixer((s) => s.playerTick);
  const playing = tick?.playing ?? player.playing;
  const pos = tick && (tick.playing || !player.playing) ? tick.position : player.position;
  const dur = song.seconds || 1;
  const addRef = useRef<HTMLInputElement>(null);
  const edit = () => {
    const name = prompt('Nome da música', song.name);
    if (name === null) return;
    const bpm = prompt('BPM (deixe vazio se não souber)', song.bpm ? String(song.bpm) : '');
    if (bpm === null) return;
    const key = prompt('Tom (ex.: Sol, Ré#m)', song.key);
    if (key === null) return;
    run(playerApi.update(song.id, { name, bpm: bpm.trim() ? Number(bpm.replace(',', '.')) : null, key }));
  };
  const del = () => {
    if (confirm(`Apagar “${song.name}” e os arquivos dela deste computador?`)) run(playerApi.remove(song.id));
  };
  const iconBtn = 'grid h-11 w-11 place-items-center rounded-xl border border-line bg-panel-2 text-text hover:border-line-hi disabled:opacity-40';
  return (
    <Card title="Tocando agora" right={
      <span className="flex gap-1">
        <Btn variant="ghost" onClick={() => addRef.current?.click()}><Plus size={14} weight="bold" /> Faixas</Btn>
        <Btn variant="ghost" onClick={edit}><PencilSimple size={14} weight="bold" /> Editar</Btn>
        <Btn variant="ghost" onClick={del} aria-label="Apagar música"><Trash size={14} weight="bold" /></Btn>
        <input ref={addRef} type="file" multiple accept=".wav,.wave,.aif,.aiff,.flac,.mp3,audio/*" className="hidden" onChange={(e) => {
          const f = Array.from(e.target.files ?? []);
          e.target.value = '';
          if (f.length) onAdd(f);
        }} />
      </span>
    }>
      <div className="flex flex-wrap items-end gap-4">
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <h1 className="truncate text-[24px] font-extrabold tracking-tight text-hi">{song.name}</h1>
          <span className="text-[12.5px] text-subtle">
            {[song.bpm ? `${song.bpm} BPM` : null, song.key || null, `${song.tracks.length} faixas`].filter(Boolean).join(' · ')}
          </span>
        </div>
        <div className="flex items-center gap-2">
          <button type="button" aria-label="Música anterior" className={iconBtn} onClick={() => run(playerApi.action('prev'))}>
            <SkipBack size={18} weight="fill" />
          </button>
          <button
            type="button"
            aria-label={playing ? 'Pausar' : 'Tocar'}
            disabled={!song.tracks.length}
            onClick={() => run(playerApi.action(playing ? 'pause' : 'play'))}
            className="grid h-14 w-14 place-items-center rounded-2xl bg-accent text-white shadow-[var(--shadow-primary)] hover:brightness-110 disabled:opacity-40"
          >
            {playing ? <Pause size={24} weight="fill" /> : <Play size={24} weight="fill" />}
          </button>
          <button type="button" aria-label="Parar e voltar ao início" className={iconBtn} onClick={() => run(playerApi.action('stop'))}>
            <Stop size={16} weight="fill" />
          </button>
          <button type="button" aria-label="Próxima música" className={iconBtn} onClick={() => run(playerApi.action('next'))}>
            <SkipForward size={18} weight="fill" />
          </button>
        </div>
      </div>
      <button
        type="button"
        aria-label={`Posição ${fmtTime(pos)} de ${fmtTime(dur)}. Clique para pular.`}
        onClick={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          run(playerApi.action('seek', { seconds: ((e.clientX - r.left) / r.width) * dur }));
        }}
        className="relative mt-1 h-12 w-full overflow-hidden rounded-[10px] border border-line bg-deep"
      >
        <span className="absolute inset-y-0 left-0 bg-accent/20" style={{ width: `${Math.min(100, (pos / dur) * 100)}%` }} />
        <span className="absolute inset-y-0 w-0.5 bg-cyan" style={{ left: `${Math.min(100, (pos / dur) * 100)}%` }} />
      </button>
      <div className="flex justify-between font-mono text-[12px] text-subtle">
        <span className="text-text">{fmtTime(pos)}</span>
        <span>Clique na barra para pular</span>
        <span>{fmtTime(dur)}</span>
      </div>
    </Card>
  );
}

/** Fader horizontal que manda o valor só ao soltar (e mostra enquanto arrasta). */
function GainSlider({ label, value, onCommit }: { label: string; value: number; onCommit: (v: number) => void }) {
  const [v, setV] = useState(value);
  const dragging = useRef(false);
  useEffect(() => {
    if (!dragging.current) setV(value);
  }, [value]);
  const commit = () => {
    dragging.current = false;
    if (v !== value) onCommit(v);
  };
  return (
    <span className="flex min-w-0 flex-1 items-center gap-2.5">
      <input
        type="range"
        aria-label={label}
        min={-60}
        max={10}
        step={0.5}
        value={v}
        onPointerDown={() => (dragging.current = true)}
        onChange={(e) => setV(Number(e.target.value))}
        onPointerUp={commit}
        onKeyUp={commit}
        onBlur={commit}
        onDoubleClick={() => onCommit(0)}
        className="h-1.5 min-w-0 flex-1 cursor-pointer accent-accent"
      />
      <span className="w-14 text-right font-mono text-[11.5px] text-subtle">{fmtDb(v)}</span>
    </span>
  );
}

function TrackMeter({ k }: { k: number }) {
  const peak = useMixer((s) => s.playerTick?.peaks[k] ?? 0);
  const w = peak > 0 ? Math.max(4, Math.min(100, 100 + (20 * Math.log10(peak) * 100) / 60)) : 0;
  return (
    <span className="block h-2 w-16 overflow-hidden rounded-full bg-deep" aria-hidden="true">
      <span className={cx('block h-full rounded-full', peak >= 1 ? 'bg-live' : 'bg-ok')} style={{ width: `${w}%` }} />
    </span>
  );
}

function Tracks({ player, song, run }: { player: PlayerState; song: PlayerSong; run: (p: Promise<unknown>) => void }) {
  const split = player.mode === 'split';
  const set = (t: PlayerTrack, patch: Partial<PlayerTrack>) => run(playerApi.update(song.id, { tracks: [{ file: t.file, ...patch }] }));
  const rename = (t: PlayerTrack) => {
    const n = prompt('Nome da faixa', t.name);
    if (n) set(t, { name: n });
  };
  const seg = (on: boolean) => cx('h-8 rounded-lg px-3 text-[12.5px] font-bold transition', on ? 'bg-accent text-white' : 'text-subtle hover:text-text');
  return (
    <Card
      title="Faixas"
      right={
        <span className="flex items-center gap-2">
          <span className="text-[12px] text-subtle">Saída</span>
          <span role="group" aria-label="Modo de saída" className="flex rounded-[10px] border border-line bg-deep p-0.5">
            <button type="button" aria-pressed={!split} className={seg(!split)} onClick={() => run(playerApi.action('settings', { mode: 'simple' }))}>Canal único</button>
            <button type="button" aria-pressed={split} className={seg(split)} onClick={() => run(playerApi.action('settings', { mode: 'split' }))}>Faixa por canal</button>
          </span>
        </span>
      }
    >
      <p className="text-[12.5px] leading-relaxed text-subtle">
        {split ? (
          <>Cada faixa vira <b className="text-text">um canal da mesa do Sona</b> (“MT …”, tag multitrack), com EQ, compressor e envios próprios.</>
        ) : (
          <>As faixas são somadas aqui e entram na mesa do Sona como <b className="text-text">um canal estéreo, “Player”</b>.</>
        )}{' '}
        Faixas com <Headphones size={13} weight="bold" className="inline text-cyan" /> ligado (click, guia) não entram na soma nem na transmissão: vão para “Click + guia”, que só toca nos fones.
      </p>
      {song.tracks.length === 0 ? (
        <p className="py-4 text-center text-[13px] text-muted">Sem faixas. Use “+ Faixas” acima.</p>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full min-w-[640px] border-collapse text-left">
            <thead>
              <tr className="border-b border-line text-[11px] font-bold uppercase tracking-[0.06em] text-muted">
                <th className="py-2 pr-2 font-bold">Faixa</th>
                <th className="py-2 pr-2 font-bold">Sinal</th>
                <th className="py-2 pr-2 font-bold">Volume</th>
                <th className="py-2 pr-2 font-bold">M · S · Fone</th>
                <th className="py-2 pr-2 font-bold">Vai para</th>
                <th className="py-2" />
              </tr>
            </thead>
            <tbody>
              {song.tracks.map((t, k) => (
                <tr key={t.file} className="border-b border-panel-2">
                  <td className="max-w-[180px] py-1.5 pr-2">
                    <button type="button" onClick={() => rename(t)} title={`${t.file} · ${t.channels > 1 ? 'estéreo' : 'mono'} · ${(t.sample_rate / 1000).toFixed(1).replace('.', ',')} kHz. Clique para renomear.`} className="flex max-w-full items-center gap-2 text-left">
                      <span className={cx('h-2 w-2 shrink-0 rounded-full', t.cue ? 'bg-cyan' : 'bg-accent-light')} />
                      <span className="truncate text-[13px] font-bold text-text">{t.name}</span>
                    </button>
                  </td>
                  <td className="py-1.5 pr-2"><TrackMeter k={k} /></td>
                  <td className="w-[38%] py-1.5 pr-2">
                    <GainSlider label={`Volume de ${t.name}`} value={t.gain_db} onCommit={(v) => set(t, { gain_db: v })} />
                  </td>
                  <td className="py-1.5 pr-2">
                    <span className="flex gap-1">
                      <Toggle tone="warn" on={t.mute} aria-label={`Silenciar ${t.name}`} onClick={() => set(t, { mute: !t.mute })} className="w-8">M</Toggle>
                      <Toggle tone="cyan" on={t.solo} aria-label={`Solo de ${t.name}`} onClick={() => set(t, { solo: !t.solo })} className="w-8">S</Toggle>
                      <Toggle tone="accent" on={t.cue} aria-label={`${t.name} só no fone`} title="Só no fone (click, guia)" onClick={() => set(t, { cue: !t.cue })} className="grid w-8 place-items-center">
                        <Headphones size={13} weight="bold" />
                      </Toggle>
                    </span>
                  </td>
                  <td className="py-1.5 pr-2">
                    <span className={cx('inline-block max-w-[180px] truncate rounded-lg px-2 py-1 text-[12px] font-semibold', t.cue ? 'bg-cyan/10 text-cyan' : 'bg-accent/15 text-accent-text')}>
                      {t.cue ? 'Fones · só monitor' : split ? `Canal “MT ${t.name}”` : 'Player (soma)'}
                    </span>
                  </td>
                  <td className="py-1.5">
                    <button type="button" aria-label={`Tirar a faixa ${t.name}`} onClick={() => confirm(`Tirar “${t.name}” desta música?`) && run(playerApi.removeTrack(song.id, t.file))} className="grid h-7 w-7 place-items-center rounded-md text-muted hover:bg-panel-2 hover:text-text">
                      <X size={13} weight="bold" />
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </Card>
  );
}

function DeskSummary({ player, song }: { player: PlayerState; song: PlayerSong | null }) {
  const st = useMixer((s) => s.state);
  const chans = (st?.mixer.channels ?? []).filter((c) => c.tags?.includes('multitrack'));
  return (
    <Card title="Na mesa do Sona">
      {chans.length === 0 ? (
        <p className="text-[12px] text-muted">Os canais aparecem quando houver uma música.</p>
      ) : (
        <ul className="flex flex-col gap-1.5">
          {chans.map((c) => {
            const cue = c.name === 'Click + guia';
            return (
              <li key={c.name} className="flex items-center gap-2.5 rounded-[10px] border border-line bg-panel-2 px-2.5 py-2">
                <span className={cx('grid h-8 w-8 place-items-center rounded-lg text-[10.5px] font-extrabold', cue ? 'bg-cyan/15 text-cyan' : 'bg-accent/20 text-accent-text')}>
                  {cue ? <Headphones size={15} weight="bold" /> : c.source.kind === 'stereo' ? 'L R' : 'M'}
                </span>
                <span className="flex min-w-0 flex-1 flex-col">
                  <span className="truncate text-[13px] font-bold">{c.name}</span>
                  <span className="text-[11px] text-muted">{cue ? 'Só fones · nunca na transmissão' : c.source.kind === 'stereo' ? 'Estéreo' : 'Mono'}</span>
                </span>
                <span className="font-mono text-[11.5px] text-subtle">{fmtDb(c.fader_db)}</span>
              </li>
            );
          })}
        </ul>
      )}
      {player.mode === 'split' && song && (
        <p className="text-[11.5px] leading-snug text-muted">Ao trocar de música, cada canal “MT” passa a ser a faixa na mesma posição da música nova (ajustes ficam no canal).</p>
      )}
    </Card>
  );
}

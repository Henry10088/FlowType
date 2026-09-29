import { useEffect, useRef, useState } from 'react';
import { ArrowsLeftRight, CheckCircle, Circle, Copy, DeviceMobile, File, FolderOpen, GearSix, Info, Minus, Monitor, PencilSimpleLine, Plus, ShieldCheck, Trash, UploadSimple, WarningCircle, X } from '@phosphor-icons/react';
import { QRCodeCanvas } from 'qrcode.react';

const nav = [['status', '状态', Monitor], ['files', '文件传输', ArrowsLeftRight], ['phones', '已绑定手机', DeviceMobile], ['settings', '设置', GearSix]];
const initialPhones = [{ id: 1, name: '我的手机', online: true, detail: '刚刚连接' }, { id: 2, name: '备用手机', online: false, detail: '上次连接：昨天 21:08' }];

function Button({ children, icon: Icon, tone = 'neutral', ...props }) {
  return <button className={`button ${tone}`} {...props}>{Icon && <Icon size={17} aria-hidden="true" />}{children}</button>;
}
function Dot({ online = true }) { return <span className={`dot ${online ? '' : 'offline'}`} aria-hidden="true" />; }

export function App() {
  const [page, setPage] = useState('status');
  const [phones, setPhones] = useState(initialPhones);
  const [back, setBack] = useState('phones');
  const [unbind, setUnbind] = useState(null);
  const [toast, setToast] = useState('');
  const [connected, setConnected] = useState(true);
  const [tab, setTab] = useState('send');
  const [files, setFiles] = useState([]);
  const [progress, setProgress] = useState(null);
  const [drag, setDrag] = useState(false);
  const [name, setName] = useState('办公室电脑');
  const [savedName, setSavedName] = useState('办公室电脑');
  const [startup, setStartup] = useState(true);
  const [update, setUpdate] = useState('检查更新');
  const [service, setService] = useState('正常运行');
  const input = useRef(null), folder = useRef(null), toastTimer = useRef(null);

  const notice = text => { setToast(text); clearTimeout(toastTimer.current); toastTimer.current = setTimeout(() => setToast(''), 3000); };
  useEffect(() => () => clearTimeout(toastTimer.current), []);
  useEffect(() => {
    if (progress === null || progress >= 100) return;
    const timer = setTimeout(() => setProgress(Math.min(100, progress + 8)), 280);
    return () => clearTimeout(timer);
  }, [progress]);

  const pairing = () => { setBack(page); setPage('pairing'); };
  const addFiles = list => {
    const next = Array.from(list).map((file, i) => ({ id: `${Date.now()}-${i}`, name: file.webkitRelativePath || file.name, size: file.size }));
    setFiles(old => [...old, ...next]); setTab('send');
  };
  const size = bytes => bytes >= 1048576 ? `${(bytes / 1048576).toFixed(1)} MB` : `${Math.max(1, Math.round(bytes / 1024))} KB`;

  return <div className="stage">
    <div className="stage-note">Windows 界面原型 <span>·</span> 演示数据，不连接真实设备</div>
    <div className="window">
      <header className="titlebar"><div className="brand"><span className="brand-mark" aria-hidden="true"><PencilSimpleLine className="brand-pencil" size={19} weight="regular" /><Circle className="brand-dot" size={6} weight="fill" /></span><strong>说写</strong><small>FlowType</small></div><div className="window-buttons" aria-hidden="true"><Minus size={16} /><X size={17} /></div></header>
      <div className="workspace"><aside className="sidebar"><div className="nav-caption">工作空间</div><nav aria-label="主导航">{nav.map(([key, label, Icon]) => <button key={key} className={`nav-item ${page === key || (page === 'pairing' && key === 'phones') ? 'active' : ''}`} aria-current={page === key ? 'page' : undefined} onClick={() => setPage(key)}><Icon size={20} weight={page === key ? 'fill' : 'regular'} /><span>{label}</span>{key === 'phones' && <small>{phones.length}</small>}</button>)}</nav><div className="sidebar-foot"><ShieldCheck size={17} />已加密连接</div></aside>
      <main className="content">
        {page === 'status' && <><div className="heading"><span className="eyebrow">连接概览</span><h1>状态</h1></div>
          <div className="hero"><div className="hero-icon"><Monitor size={29} weight="light" /></div><div className="hero-copy"><strong>{connected ? '已准备好接收手机输入' : '正在重新连接手机'}</strong><p>{connected ? '在手机上打开说写，文字会出现在电脑当前光标处。' : '手机上仍可继续编辑，连接恢复后同步最新内容。'}</p></div><span className={`pill ${connected ? '' : 'waiting'}`}><Dot online={connected} />{connected ? '运行正常' : '重新连接中'}</span></div>
          <div className="info-list"><div className="info-row"><label>连接手机</label><strong><Dot online={connected} />{connected ? '我的手机' : '暂无连接'}</strong><small>{connected ? '刚刚连接' : '已绑定手机可自动重连'}</small></div><div className="info-row"><label>输入状态</label><strong>等待手机开始输入</strong></div><div className="info-row"><label>当前目标</label><strong>尚未开始输入</strong></div><div className="info-row"><label>局域网地址</label><strong>192.168.1.23</strong><button className="icon-button" title="复制局域网地址" aria-label="复制局域网地址" onClick={() => { navigator.clipboard?.writeText('192.168.1.23'); notice('地址已复制'); }}><Copy size={17} /></button></div></div>
          <div className="actions"><Button icon={Plus} onClick={pairing}>绑定新手机</Button><button className="text-button" onClick={() => setConnected(!connected)}>预览{connected ? '重连' : '正常'}状态 →</button></div></>}

        {page === 'phones' && <><div className="heading heading-action"><div><span className="eyebrow">设备管理</span><h1>已绑定手机</h1></div><Button tone="primary" icon={Plus} onClick={pairing}>绑定新手机</Button></div><p className="intro">手机绑定一次即可使用，连接断开后会自动重连。</p><div className="phone-list">{phones.map(phone => <div className="phone-row" key={phone.id}><div className="phone-icon"><DeviceMobile size={23} weight="light" /></div><div className="phone-text"><strong>{phone.name}</strong><small><Dot online={phone.online} />{phone.online ? '已连接' : '未连接'} <em>·</em> {phone.detail}</small></div><Button tone="quiet" onClick={() => setUnbind(phone)}>解除绑定</Button></div>)}</div><p className="hint"><Info size={16} />解除绑定后，需要重新扫码才能在这台电脑上使用。</p></>}

        {page === 'pairing' && <><div className="heading pairing-heading"><div><h1>绑定新手机</h1><p className="intro">在说写 Android 应用中，打开「添加电脑」扫描二维码。</p></div><button className="back" onClick={() => setPage(back)}>返回手机列表</button></div><div className="pair-box"><div className="qr"><QRCodeCanvas value="flowtype://pair?prototype=windows-ui" size={168} marginSize={1} fgColor="#1d3235" /><small>仅供界面演示</small></div><div className="steps"><div><b>1</b><span><strong>打开手机上的说写</strong><small>在电脑列表中选择「添加电脑」。</small></span></div><div><b>2</b><span><strong>扫描左侧二维码</strong><small>绑定后就能输入文字和传输文件。</small></span></div><Button tone="primary" onClick={() => { if (!phones.some(p => p.id === 3)) setPhones([...phones, { id: 3, name: '新绑定的手机', online: true, detail: '刚刚绑定' }]); setPage('phones'); notice('新手机已绑定'); }}>模拟扫码成功</Button></div></div><p className="hint"><ShieldCheck size={16} />绑定长期有效，可随时手动解除。</p></>}

        {page === 'files' && <><div className="heading"><span className="eyebrow">双向传输</span><h1>文件传输</h1><p className="intro">与已绑定手机互传文件和文件夹，断线后自动续传。</p></div><div className="tabs" role="tablist" aria-label="文件传输"><button role="tab" aria-selected={tab === 'send'} className={tab === 'send' ? 'selected' : ''} onClick={() => setTab('send')}>发送到手机</button><button role="tab" aria-selected={tab === 'received'} className={tab === 'received' ? 'selected' : ''} onClick={() => setTab('received')}>收到的文件</button></div>
          {tab === 'send' ? <><div className="recipient">发送到 <strong><Dot />我的手机</strong><span>已连接</span></div><div className={`dropzone ${drag ? 'drag' : ''}`} onDragOver={e => { e.preventDefault(); setDrag(true); }} onDragLeave={e => { e.preventDefault(); setDrag(false); }} onDrop={e => { e.preventDefault(); setDrag(false); addFiles(e.dataTransfer.files); }}><div className="drop-icon"><UploadSimple size={25} weight="light" /></div><strong>将文件或文件夹拖到这里</strong><small>也可以从电脑中选择；此原型不会真正上传</small><div><Button icon={File} onClick={() => input.current?.click()}>选择文件</Button><Button icon={FolderOpen} onClick={() => folder.current?.click()}>选择文件夹</Button></div><input ref={input} type="file" multiple hidden onChange={e => { addFiles(e.target.files); e.target.value = ''; }} /><input ref={folder} type="file" webkitdirectory="" directory="" multiple hidden onChange={e => { addFiles(e.target.files); e.target.value = ''; }} /></div>
            {(files.length > 0 || progress !== null) && <div className="queue"><div className="queue-head"><strong>{progress !== null ? '传输进度' : `待发送 · ${files.length} 项`}</strong>{progress === null && <button className="text-button" onClick={() => setFiles([])}>清空</button>}</div>{progress !== null ? <><div className="transfer-line"><span>{progress === 100 ? '已完成' : '正在发送'} · {files.length} 项</span><strong>{progress}%</strong></div><div className="progress"><span style={{ width: `${progress}%` }} /></div><div className="transfer-line secondary"><span>{progress === 100 ? '文件已发送到手机' : '加密传输中'}</span><button className="text-button" onClick={() => { setProgress(null); setFiles([]); }}>{progress === 100 ? '完成' : '取消'}</button></div></> : <><div className="queue-items">{files.slice(0, 3).map(file => <div key={file.id}><File size={16} /><span title={file.name}>{file.name}</span><small>{size(file.size)}</small><button aria-label={`移除 ${file.name}`} onClick={() => setFiles(files.filter(f => f.id !== file.id))}><X size={14} /></button></div>)}{files.length > 3 && <small>还有 {files.length - 3} 项文件</small>}</div><Button tone="primary" icon={UploadSimple} onClick={() => setProgress(0)}>发送到我的手机</Button></>}</div>}</> : <><div className="received-intro"><span className="drop-icon"><FolderOpen size={23} /></span><div><strong>手机发来的文件会自动保存</strong><small>保存位置：下载 / FlowType</small></div><Button icon={FolderOpen} onClick={() => notice('原型中不打开真实文件夹')}>打开文件夹</Button></div><div className="received-file"><File size={19} /><div><strong>会议纪要.pdf</strong><small>我的手机 · 今天 10:24</small></div><span>2.4 MB</span><CheckCircle size={18} weight="fill" /></div></>}
        </>}

        {page === 'settings' && <><div className="heading"><span className="eyebrow">偏好与维护</span><h1>设置</h1></div><section className="setting-section"><h2>常规</h2><div className="setting-row"><label htmlFor="pc-name">电脑名称</label><div><input id="pc-name" value={name} maxLength={32} onChange={e => setName(e.target.value)} /><Button disabled={!name.trim() || name === savedName} onClick={() => { setSavedName(name.trim()); setName(name.trim()); notice('电脑名称已保存'); }}>保存</Button></div></div><div className="setting-row"><span>开机启动</span><div className="setting-value"><span>登录 Windows 后自动启动说写</span><button className={`switch ${startup ? 'on' : ''}`} role="switch" aria-checked={startup} aria-label="登录 Windows 后自动启动说写" onClick={() => { setStartup(!startup); notice(startup ? '已关闭开机启动' : '已开启开机启动'); }}><span /></button></div></div></section><section className="setting-section"><h2>输入服务</h2><div className="setting-row"><span>运行状态</span><div className="setting-value"><strong><Dot />{service}</strong><small>可向普通和管理员窗口输入文字</small><Button disabled={service !== '正常运行'} onClick={() => { setService('正在修复'); setTimeout(() => { setService('正常运行'); notice('输入服务已恢复正常'); }, 1200); }}>修复输入服务</Button></div></div></section><section className="setting-section"><h2>版本与更新</h2><div className="setting-row"><span>当前版本</span><div className="setting-value"><strong>0.3.0</strong><small>Windows x64</small></div></div><div className="setting-row"><span>软件更新</span><div className="setting-value"><small>{update === '检查更新' ? '保持软件为最新版本' : update}</small><Button disabled={update === '正在检查…'} onClick={() => { setUpdate('正在检查…'); setTimeout(() => setUpdate('已是最新版本'), 1100); }}>{update === '已是最新版本' ? '再次检查' : '检查更新'}</Button></div></div></section></>}
      </main></div></div>
    {unbind && <div className="overlay" onMouseDown={e => { if (e.target === e.currentTarget) setUnbind(null); }}><div className="dialog" role="dialog" aria-modal="true" aria-labelledby="dialog-title"><div className="warn"><WarningCircle size={24} /></div><h2 id="dialog-title">解除绑定「{unbind.name}」？</h2><p>这部手机将无法继续向本电脑输入或传文件。以后仍可以重新扫码绑定。</p><div><Button onClick={() => setUnbind(null)}>取消</Button><Button tone="danger" icon={Trash} onClick={() => { setPhones(phones.filter(p => p.id !== unbind.id)); setUnbind(null); notice('已解除绑定'); }}>解除绑定</Button></div></div></div>}
    {toast && <div className="toast" role="status"><CheckCircle size={18} weight="fill" />{toast}</div>}
  </div>;
}

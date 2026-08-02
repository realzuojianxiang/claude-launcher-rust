// 顶部状态横幅：展示后端返回的成功/错误消息
// 从 App.tsx 抽出。msg 为空时返回 null；按首字符 emoji 决定颜色。

export function MessageBanner({ msg }: { msg: string | null }) {
  if (!msg) return null;
  const ok = msg.startsWith("✅");
  const warn = msg.startsWith("⚠");
  const color = ok ? "#34c759" : warn ? "#ff9500" : "#ff3b30";
  return (
    <div style={{ color, fontSize: 13, margin: "0 0 12px", fontWeight: 600 }}>
      {msg}
    </div>
  );
}

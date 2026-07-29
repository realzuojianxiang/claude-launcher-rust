// 变量值输入框：敏感字段默认以 password 形态脱敏（带 👁 切换显隐），
// 真实值始终留在 value 中，保存时透传真实值。
// 注意：脱敏只发生在「显示层」（input type=password + 👁 切换），
// React state 中始终保存真实值，保存时原样落盘——绝不把脱敏串写进配置。
// 从 App.tsx 抽出，自带 React 导入。

import { useState } from "react";

// 判断环境变量键名是否为敏感字段：键名以 AUTH_TOKEN/API_KEY 结尾或含 SECRET/PASSWORD/PRIVATE_KEY
export const isSecretKey = (k: string): boolean =>
  /AUTH_TOKEN$|API_KEY$|SECRET|PASSWORD|PRIVATE_KEY/i.test(k);

export function EnvValueInput({
  value,
  secret,
  placeholder,
  onChange,
}: {
  value: string;
  secret: boolean;
  placeholder?: string;
  onChange: (v: string) => void;
}) {
  const [reveal, setReveal] = useState(false);
  if (!secret) {
    return (
      <input
        type="text"
        className="env-val"
        value={value}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
      />
    );
  }
  return (
    <span className="env-val-secret">
      <input
        type={reveal ? "text" : "password"}
        className="env-val"
        value={value}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
      />
      <button
        type="button"
        className="env-reveal"
        title={reveal ? "隐藏" : "显示真实值"}
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => setReveal((r) => !r)}
      >
        {reveal ? "🙈" : "👁"}
      </button>
    </span>
  );
}

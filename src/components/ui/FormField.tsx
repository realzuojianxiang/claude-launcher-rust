export interface FormFieldProps {
  id: string;
  label: string;
  hint?: string;
  error?: string;
  success?: string;
  status?: "default" | "success" | "error";
  required?: boolean;
  children: (describedBy: string | undefined) => React.ReactNode;
}

export function FormField({
  id,
  label,
  hint,
  error,
  success,
  status = "default",
  required,
  children,
}: FormFieldProps) {
  const ids: string[] = [];
  if (hint) ids.push(`${id}-hint`);
  if (error) ids.push(`${id}-error`);
  if (success) ids.push(`${id}-success`);
  const describedBy = ids.length > 0 ? ids.join(" ") : undefined;

  return (
    <div className={`form-field form-field--${status}`}>
      <label htmlFor={id} className="form-field__label">
        {label}
        {required ? <span aria-hidden="true"> *</span> : null}
      </label>
      {hint ? (
        <p id={`${id}-hint`} className="form-field__hint">
          {hint}
        </p>
      ) : null}
      <div className="form-field__control">{children(describedBy)}</div>
      {error ? (
        <p id={`${id}-error`} className="form-field__error" role="alert">
          {error}
        </p>
      ) : null}
      {success ? (
        <p id={`${id}-success`} className="form-field__success" role="status">
          {success}
        </p>
      ) : null}
    </div>
  );
}

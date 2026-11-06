type TabsProps<T extends string> = {
  value: T;
  options: readonly { value: T; label: string }[];
  onChange: (value: T) => void;
  /** The accessible name of the tab list. */
  name: string;
};

/** A row of equal-width tabs that picks which view a panel shows. */
export function Tabs<T extends string>({ value, options, onChange, name }: TabsProps<T>) {
  return <div className="kit-tabs" role="tablist" aria-label={name}>
    {options.map((option) => <button
      key={option.value}
      type="button"
      role="tab"
      className="kit-tab"
      aria-selected={option.value === value}
      onClick={() => { onChange(option.value); }}
    >{option.label}</button>)}
  </div>;
}

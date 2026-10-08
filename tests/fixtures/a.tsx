import React from "react";

export function App(): JSX.Element {
  return <div>hi</div>;
}

export const Button = (props: { label: string }) => <button>{props.label}</button>;

import { Icon } from "../../ui/Icon";

export function DropView() {
  return (
    <div className="view view-drop">
      <div className="drop-zone">
        <Icon name="drop" size={20} />
        <span>Отпустите, чтобы передать Джарвису</span>
      </div>
    </div>
  );
}

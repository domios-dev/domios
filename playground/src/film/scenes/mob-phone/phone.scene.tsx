import { sequence, type Scene } from "../../engine";
import { PhoneShot, phoneBeats, phoneBeatId } from "./PhoneShot";

const shots: Scene[] = phoneBeats.map(beat => ({
  id: phoneBeatId(beat), duration: 6, render: t => <PhoneShot t={t} beat={beat} />,
}));

export default [...shots, sequence("mob-phone-reel", shots)];

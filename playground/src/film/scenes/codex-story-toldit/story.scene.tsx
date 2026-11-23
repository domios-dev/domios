import { sequence } from "../../engine";
import chat from "./chat.scene";
import team from "./team.scene";
import render from "./render.scene";
import review from "./review.scene";
import merge from "./merge.scene";
import payoff from "./payoff.scene";

export default sequence("cx-story-toldit-story", [chat, team, render, review, merge, payoff]);

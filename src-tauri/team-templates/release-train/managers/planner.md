Shape the release of the `version` input on the `release_branch` input into a short plan the Conductor can run as a train.

Start from the project's release and gates procedures: list each gate the release kind requires, then attach each gate as acceptance to the task that produces its evidence. Typical tasks: candidate build and gates (build owner), live proof on the candidate SHA (QA), CI, qualification and the release PR (release manager), and publication behind the owner's GO.

Every task names its observable pass condition and the SHA it applies to. Publication is its own task, blocked on the owner's GO, never folded into qualification. Do not add implementation tasks: code fixes are outside this Team and go to the owner.

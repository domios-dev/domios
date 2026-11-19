import { RELEASE_BRANCH } from './release-version.mjs';
import {verifyQualification, selectQualificationArtifact, retainQualificationEvidence, qualificationAttemptNumbers, selectReleasePushRun} from './ci-qualification.mjs';
import {readQualificationArchive} from './ci-qualification-archive.mjs';
import {parseTestWaiver, testWaiverEvidence} from './release-test-waiver.mjs';
export {readQualificationArchive} from './ci-qualification-archive.mjs';

export async function verifyIssueReferences(github, selection) {
  for (const issue of [...selection.issues,...selection.repository_issues]) {
    const match=/^([^/]+)\/([^#]+)#(\d+)$/.exec(issue.reference);
    if(!match)throw new Error('Invalid declared issue reference: '+issue.reference);
    await github.rest.issues.get({owner:match[1],repo:match[2],issue_number:Number(match[3])});
  }
}
export async function listFailureJobs(github, context) {
  const jobs=await github.paginate(github.rest.actions.listJobsForWorkflowRunAttempt,{
    ...context.repo,run_id:context.runId,attempt_number:Number(process.env.GITHUB_RUN_ATTEMPT),per_page:100});
  return jobs.filter(job=>job.conclusion&&!['success','skipped'].includes(job.conclusion)).map(job=>job.name+': '+job.conclusion);
}
export async function cancelWorkflow(github, context) {
  await github.rest.actions.cancelWorkflowRun({...context.repo,run_id:context.runId});
}
export async function requireQualificationWithGithub(github, context, sha, {runId,
  waiver = process.env.RELEASE_TEST_WAIVER ? JSON.parse(process.env.RELEASE_TEST_WAIVER) : undefined,
  readArchive=readQualificationArchive, retain=retainQualificationEvidence} = {}) {
  if(!/^[a-f0-9]{40}$/.test(sha))throw new Error('Invalid qualification SHA');
  if(runId !== undefined && !/^[1-9]\d*$/.test(String(runId)))throw new Error('Invalid qualification run ID');
  if (waiver) {
    if (String(runId) !== waiver.run_id) throw new Error('Waiver belongs to a different run');
    const publicationRecovery = context.eventName === 'push' && context.ref === 'refs/heads/master' &&
      context.actor === 'rollacode' && process.env.GITHUB_TRIGGERING_ACTOR === 'rollacode';
    const reader = testWaiverEvidence(sha, waiver, context.repo.owner+'/'+context.repo.repo, publicationRecovery);
    let next = reader.next();
    while (!next.done) {
      const {route, raw, optional} = next.value;
      let value;
      try {
        value = (await github.request('GET /'+route, raw ? {headers:{accept:'application/octet-stream'}} : {})).data;
        if (raw) value = typeof value === 'string' ? value : Buffer.from(value).toString('utf8');
      } catch (error) {
        if (!optional || error.status !== 404) throw error;
        value = null;
      }
      next = reader.next(value);
    }
    return retain(next.value);
  }
  const name='qualification-'+sha;
  const artifacts=(await (runId === undefined
    ? github.rest.actions.listArtifactsForRepo({...context.repo,name,per_page:100})
    : github.rest.actions.listWorkflowRunArtifacts({...context.repo,run_id:runId,name,per_page:100}))).data.artifacts;
  const artifact=selectQualificationArtifact(artifacts,sha,Date.now(),runId);
  const run=(await github.rest.actions.getWorkflowRun({...context.repo,run_id:artifact.workflow_run.id})).data;
  if(String(run.id)!==String(artifact.workflow_run.id))throw new Error('Foreign qualification artifact run');
  const jobs=[];
  for(let page=1;;page++){
    if(page>20)throw new Error('Qualification job inventory exceeds bound');
    const batch=(await github.rest.actions.listJobsForWorkflowRun({...context.repo,run_id:run.id,filter:'all',per_page:100,page})).data;
    jobs.push(...batch.jobs);if(jobs.length>=batch.total_count)break;
    if(!batch.jobs.length)throw new Error('Incomplete qualification job inventory');
  }
  const attempts=[];
  for(const number of qualificationAttemptNumbers(run)){
    attempts.push((await github.rest.actions.getWorkflowRunAttempt({...context.repo,run_id:run.id,attempt_number:number})).data);
  }
  const archive=(await github.rest.actions.downloadArtifact({...context.repo,artifact_id:artifact.id,archive_format:'zip'})).data;
  return retain({...verifyQualification(readArchive(archive),run,jobs,sha,context.repo.owner+'/'+context.repo.repo,attempts),
    verification_artifact:{id:artifact.id,expires_at:artifact.expires_at}});
}

export async function requireReleasePrQualification(github, context, verify = requireQualificationWithGithub) {
  const pr = context.payload.pull_request;
  const repository = context.repo.owner + '/' + context.repo.repo;
  if (context.eventName !== 'pull_request' || pr?.base.ref !== 'master' || pr.user.login !== 'rollacode'
    || pr.head.repo.full_name !== repository || pr.head.ref !== RELEASE_BRANCH)
    throw new Error('master accepts only an owner release/next PR');
  const current = (await github.rest.pulls.get({...context.repo, pull_number: pr.number})).data;
  const branch = (await github.rest.repos.getBranch({...context.repo, branch: pr.head.ref})).data;
  if (current.head.sha !== pr.head.sha || current.head.ref !== pr.head.ref || current.head.repo.full_name !== repository
    || current.base.ref !== 'master' || current.state !== 'open' || branch.commit.sha !== pr.head.sha)
    throw new Error('Superseded release PR head');
  const runs = await github.paginate(github.rest.actions.listWorkflowRuns, {...context.repo,
    workflow_id: 'tests.yml', event: 'push', head_sha: pr.head.sha, branch: pr.head.ref, per_page: 100});
  const waiver = parseTestWaiver(current.body ?? '');
  const run = selectReleasePushRun(runs, pr.head.sha, pr.head.ref, repository, waiver);
  return verify(github, context, pr.head.sha, {runId: run.id, ...(waiver ? {waiver} : {})});
}

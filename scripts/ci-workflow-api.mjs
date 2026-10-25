import {execFileSync} from 'node:child_process';
import {pythonInterpreter} from './ci-runner-paths.mjs';
import {verifyQualification} from './ci-qualification.mjs';

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
export function readQualificationArchive(archive) {
  const program="import io,json,sys,zipfile\nwith zipfile.ZipFile(io.BytesIO(sys.stdin.buffer.read())) as z:\n assert z.namelist().count('qualification.json') == 1\n assert z.getinfo('qualification.json').file_size <= 16777216\n sys.stdout.buffer.write(z.read('qualification.json'))";
  return JSON.parse(execFileSync(pythonInterpreter(process.platform,process.env),['-c',program],{
    input:Buffer.from(archive),encoding:'utf8',timeout:5000,maxBuffer:16*1024*1024}));
}
export async function requireQualificationWithGithub(github, context, sha, readArchive=readQualificationArchive) {
  if(!/^[a-f0-9]{40}$/.test(sha))throw new Error('Invalid qualification SHA');
  const name='qualification-'+sha;
  const artifacts=(await github.rest.actions.listArtifactsForRepo({...context.repo,name,per_page:100})).data.artifacts;
  const artifact=artifacts.filter(row=>row.name===name&&!row.expired).sort((a,b)=>b.id-a.id)[0];
  if(!artifact)throw new Error('No qualification receipt for '+sha);
  const run=(await github.rest.actions.getWorkflowRun({...context.repo,run_id:artifact.workflow_run.id})).data;
  const jobs=[];
  for(let page=1;;page++){
    if(page>20)throw new Error('Qualification job inventory exceeds bound');
    const batch=(await github.rest.actions.listJobsForWorkflowRunAttempt({...context.repo,run_id:run.id,attempt_number:run.run_attempt,per_page:100,page})).data;
    jobs.push(...batch.jobs);if(jobs.length>=batch.total_count)break;
  }
  const archive=(await github.rest.actions.downloadArtifact({...context.repo,artifact_id:artifact.id,archive_format:'zip'})).data;
  return verifyQualification(readArchive(archive),run,jobs,sha);
}

function refuse(message) {
  throw new Error('promotion refused: ' + message);
}

export function promotionPlan(receipt) {
  const retained = receipt.files.map(file => file.name);
  return {
    source: retained,
    mirror: retained.filter(name => !/^updater-.*\.json$/.test(name)),
  };
}

export function verifyAssetDigests(files, assets, repository) {
  for (const file of files) {
    const matching = assets.filter(asset => asset.name === file.name);
    if (matching.length !== 1) refuse(`${repository} expected one uploaded asset ${file.name}`);
    const asset = matching[0];
    if (!/^sha256:[a-f0-9]{64}$/.test(asset.digest ?? '')) {
      refuse(`${repository} ${file.name} has no valid sha256 API digest`);
    }
    if (!/^[a-f0-9]{64}$/.test(file.sha256 ?? '') || asset.digest !== `sha256:${file.sha256}` ||
        asset.size !== file.size || asset.state !== 'uploaded') {
      refuse(`${repository} ${file.name} differs from its retained receipt`);
    }
  }
}
